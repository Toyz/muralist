//! Background thumbnail fetching/generation on a small thread pool.
//!
//! Remote thumbs are downloaded from Wallhaven; local images are downscaled
//! with the `image` crate so the UI never decodes full 4K files.

use std::collections::HashSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use anyhow::Result;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;

use crate::config::cache_dir;
use crate::wallhaven::Client;

const WORKERS: usize = 12;
const LOCAL_THUMB_WIDTH: u32 = 480;

#[derive(Debug, Clone)]
pub enum Job {
    Remote {
        key: String,
        url: String,
    },
    Local {
        key: String,
        path: PathBuf,
    },
    /// A high-quality downscale of `path` that fits within `w`x`h` pixels, so
    /// the GPU never has to shrink a huge original (which aliases badly).
    Fit {
        key: String,
        path: PathBuf,
        w: u32,
        h: u32,
    },
    /// A heavily blurred copy for frosted-glass backdrops. GPUI has no blur
    /// filter, so we bake one: shrink, blur, and let the GPU stretch it.
    Blur {
        key: String,
        path: PathBuf,
    },
    /// `path` turned and mirrored, for the composer to show a picture the
    /// way it's being framed (GPUI can't rotate an image itself).
    Orient {
        key: String,
        path: PathBuf,
        orient: crate::span::Orient,
    },
    /// A coarse pixel mosaic, for spoiler-hidden cards.
    Pixelate {
        key: String,
        path: PathBuf,
    },
    /// Color X-ray: keep pixels belonging to palette cluster `index` (of
    /// `centers`, RGB 0..1), darken and desaturate the rest.
    XRay {
        key: String,
        path: PathBuf,
        centers: Vec<[f32; 3]>,
        index: usize,
    },
}

impl Job {
    fn key(&self) -> &str {
        match self {
            Job::Remote { key, .. }
            | Job::Local { key, .. }
            | Job::Fit { key, .. }
            | Job::Blur { key, .. }
            | Job::Pixelate { key, .. }
            | Job::Orient { key, .. }
            | Job::XRay { key, .. } => key,
        }
    }
}

pub type Done = (String, Result<PathBuf, String>);

/// Pending jobs, served newest first: whatever was requested last is what's
/// on screen now, while older requests may have scrolled out of view.
#[derive(Default)]
struct Stack {
    jobs: Mutex<Vec<Job>>,
    ready: Condvar,
}

impl Stack {
    fn pop(&self) -> Job {
        let mut jobs = self.jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            if let Some(job) = jobs.pop() {
                return job;
            }
            jobs = self.ready.wait(jobs).unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }
}

pub struct ThumbPool {
    stack: Arc<Stack>,
}

impl ThumbPool {
    pub fn start(client: Client) -> (Self, UnboundedReceiver<Done>) {
        let stack = Arc::new(Stack::default());
        let (done_tx, done_rx) = unbounded::<Done>();
        for _ in 0..WORKERS {
            let stack = stack.clone();
            let done_tx: UnboundedSender<Done> = done_tx.clone();
            let client = client.clone();
            thread::spawn(move || {
                loop {
                    let job = stack.pop();
                    let key = job.key().to_owned();
                    // A decoder panicking on a malformed file fails that job,
                    // not the worker (and every job it would have run).
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&client, &job)))
                        .unwrap_or_else(|_| Err(anyhow::anyhow!("thumbnail worker panicked")))
                        .map_err(|e| format!("{e:#}"));
                    if done_tx.unbounded_send((key, result)).is_err() {
                        return;
                    }
                }
            });
        }
        (Self { stack }, done_rx)
    }

    pub fn request(&self, job: Job) {
        self.stack.jobs.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(job);
        self.stack.ready.notify_one();
    }
}

fn run(client: &Client, job: &Job) -> Result<PathBuf> {
    let dir = cache_dir().join("thumbs");
    match job {
        Job::Remote { key, url } => {
            let dest = remote_cache_path(key, url);
            match reuse(&dest) {
                Some(hit) => Ok(hit),
                None => client.download(url, &dest),
            }
        }
        Job::XRay { key, path, centers, index } => {
            let dest = dir.join(format!("{}.png", sanitize(key)));
            if let Some(hit) = reuse(&dest) {
                return Ok(hit);
            }
            std::fs::create_dir_all(&dir)?;
            let mut img = image::open(path)?.to_rgb8();
            let dist =
                |a: [f32; 3], b: &[f32; 3]| (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2);
            for px in img.pixels_mut() {
                let rgb = px.0.map(|v| f32::from(v) / 255.);
                let nearest =
                    centers.iter().enumerate().min_by(|a, b| dist(rgb, a.1).total_cmp(&dist(rgb, b.1))).map(|(i, _)| i);
                if nearest != Some(*index) {
                    let lum = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
                    let v = (lum * 0.22 * 255.) as u8;
                    px.0 = [v, v, v];
                }
            }
            save_png(&dest, &img)?;
            Ok(dest)
        }
        Job::Pixelate { path, .. } => {
            let dest = dir.join(format!("pix-{:016x}.png", path_hash(path)));
            if let Some(hit) = reuse(&dest) {
                return Ok(hit);
            }
            std::fs::create_dir_all(&dir)?;
            // ~14 blocks across, enlarged with hard edges.
            let tiny = image::open(path)?.thumbnail(14, 14);
            let (w, h) = (tiny.width() * 20, tiny.height() * 20);
            save_png(&dest, &tiny.resize_exact(w, h, FilterType::Nearest).to_rgb8())?;
            Ok(dest)
        }
        Job::Blur { path, .. } => {
            let dest = dir.join(format!("blur2-{:016x}.jpg", path_hash(path)));
            if let Some(hit) = reuse(&dest) {
                return Ok(hit);
            }
            std::fs::create_dir_all(&dir)?;
            let small = image::open(path)?.thumbnail(64, 64);
            let mut soft = small.blur(4.).resize(256, 256, FilterType::Triangle).to_rgb8();
            // Darken so text over the glass stays readable even on a white
            // wallpaper; the glass is about atmosphere, not detail.
            for px in soft.pixels_mut() {
                px.0 = px.0.map(|v| (f32::from(v) * 0.36) as u8);
            }
            save_jpeg(&dest, &soft, 85)?;
            Ok(dest)
        }
        Job::Fit { path, w, h, .. } => {
            let dest = dir.join(format!("fit-{w}x{h}-{:016x}.jpg", path_hash(path)));
            if let Some(hit) = reuse(&dest) {
                return Ok(hit);
            }
            // Already small enough: the header says so without decoding it.
            let (iw, ih) = image::image_dimensions(path)?;
            if iw <= *w && ih <= *h {
                return Ok(path.clone());
            }
            std::fs::create_dir_all(&dir)?;
            let fitted = image::open(path)?.resize(*w, *h, FilterType::CatmullRom).to_rgb8();
            save_jpeg(&dest, &fitted, 92)?;
            Ok(dest)
        }
        Job::Orient { path, orient, .. } => {
            let dest = dir.join(format!(
                "orient-{}{}-{:016x}.jpg",
                orient.turns,
                if orient.mirror { "m" } else { "" },
                path_hash(path)
            ));
            if let Some(hit) = reuse(&dest) {
                return Ok(hit);
            }
            save_jpeg(&dest, &orient.apply(image::open(path)?).to_rgb8(), 90)?;
            Ok(dest)
        }
        Job::Local { path, .. } => {
            let dest = dir.join(format!("local-{:016x}.jpg", path_hash(path)));
            if let Some(hit) = reuse(&dest) {
                return Ok(hit);
            }
            std::fs::create_dir_all(&dir)?;
            let img = image::open(path)?;
            let h =
                (u64::from(img.height()) * u64::from(LOCAL_THUMB_WIDTH) / u64::from(img.width().max(1))).max(1) as u32;
            save_jpeg(&dest, &img.thumbnail(LOCAL_THUMB_WIDTH, h).to_rgb8(), 88)?;
            Ok(dest)
        }
    }
}

/// A cached file that's already there: marked as just used (its modified
/// time is the cache's "last used", since access times are often off) and
/// handed back.
pub fn reuse(dest: &Path) -> Option<PathBuf> {
    let meta = std::fs::metadata(dest).ok().filter(std::fs::Metadata::is_file)?;
    // The sweep only needs a rough "last used", so touch at most hourly.
    let stale = meta.modified().ok().and_then(|t| t.elapsed().ok()).is_none_or(|age| age.as_secs() > 3600);
    if stale && let Ok(f) = std::fs::File::options().write(true).open(dest) {
        let _ = f.set_modified(std::time::SystemTime::now());
    }
    Some(dest.to_path_buf())
}

/// Encoded in memory, then written atomically: a crash or a second worker
/// never leaves a torn image that later reads as a cache hit.
fn save_png(dest: &Path, img: &image::RgbImage) -> Result<()> {
    let mut bytes = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)?;
    crate::store::write_atomic(dest, &bytes)
}

fn save_jpeg(dest: &Path, img: &image::RgbImage, quality: u8) -> Result<()> {
    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(&mut bytes, quality).encode_image(img)?;
    crate::store::write_atomic(dest, &bytes)
}

/// Trims the thumbnail cache to `budget` bytes, least recently used first,
/// down to 90% so it doesn't sweep again on the very next file. Files in
/// `keep` (on screen this session) stay. Returns bytes freed and left.
pub fn sweep(budget: u64, keep: &HashSet<PathBuf>) -> (u64, u64) {
    sweep_dir(&cache_dir().join("thumbs"), budget, keep)
}

fn sweep_dir(dir: &Path, budget: u64, keep: &HashSet<PathBuf>) -> (u64, u64) {
    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let meta = e.metadata().ok().filter(std::fs::Metadata::is_file)?;
            Some((meta.modified().ok()?, meta.len(), e.path()))
        })
        .collect();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    if total <= budget {
        return (0, total);
    }
    let target = budget / 10 * 9;
    files.sort_by_key(|f| f.0);
    let mut freed = 0;
    for (_, len, path) in files {
        if total <= target {
            break;
        }
        if keep.contains(&path) {
            continue;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= len;
            freed += len;
        }
    }
    (freed, total)
}

/// Where a remote job's file lands in the cache.
pub fn remote_cache_path(key: &str, url: &str) -> PathBuf {
    // From remote data: only a short alphanumeric extension is trusted.
    let ext = url
        .rsplit_once('.')
        .map(|(_, e)| e)
        .filter(|e| !e.is_empty() && e.len() <= 5 && e.bytes().all(|b| b.is_ascii_alphanumeric()))
        .unwrap_or("jpg");
    cache_dir().join("thumbs").join(format!("{}.{ext}", sanitize(key)))
}

fn sanitize(key: &str) -> String {
    key.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect()
}

/// Hash of path + mtime, so an edited file gets a fresh thumbnail.
pub fn path_hash(path: &Path) -> u64 {
    let mut h = DefaultHasher::new();
    path.hash(&mut h);
    if let Ok(meta) = std::fs::metadata(path)
        && let Ok(t) = meta.modified()
    {
        t.hash(&mut h);
    }
    h.finish()
}

#[cfg(test)]
mod lru_tests {
    use super::sweep_dir;
    use std::collections::HashSet;
    use std::time::{Duration, SystemTime};

    #[test]
    fn sweep_drops_least_recently_used_first() {
        let dir = std::env::temp_dir().join(format!("muralist-lru-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let base = SystemTime::now() - Duration::from_secs(1000);
        let paths: Vec<_> = (0..5)
            .map(|i| {
                let p = dir.join(format!("f{i}"));
                std::fs::write(&p, [0u8; 100]).unwrap();
                let f = std::fs::File::options().write(true).open(&p).unwrap();
                f.set_modified(base + Duration::from_secs(i * 10)).unwrap();
                p
            })
            .collect();
        // Keep the oldest: it's on screen.
        let keep: HashSet<_> = [paths[0].clone()].into();
        let (freed, left) = sweep_dir(&dir, 300, &keep);
        assert_eq!((freed, left), (300, 200));
        let remaining: Vec<bool> = paths.iter().map(|p| p.exists()).collect();
        assert_eq!(remaining, [true, false, false, false, true]);
        // Under budget: nothing to do.
        assert_eq!(sweep_dir(&dir, 1000, &HashSet::new()), (0, 200));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
