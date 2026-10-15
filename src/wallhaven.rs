//! Minimal client for the Wallhaven v1 API: https://wallhaven.cc/help/api

use std::fs;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use anyhow::{Context as _, Result, anyhow, bail};
use reqwest::StatusCode;
use reqwest::header::{ACCEPT_RANGES, CONTENT_LENGTH, RANGE};
use serde::Deserialize;

const BASE: &str = "https://wallhaven.cc/api/v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sorting {
    Toplist,
    DateAdded,
    Random,
    Views,
    Favorites,
    Relevance,
    Hot,
}

impl Sorting {
    pub const ALL: [Sorting; 7] = [
        Sorting::DateAdded,
        Sorting::Hot,
        Sorting::Toplist,
        Sorting::Random,
        Sorting::Views,
        Sorting::Favorites,
        Sorting::Relevance,
    ];

    pub fn api(self) -> &'static str {
        match self {
            Sorting::Toplist => "toplist",
            Sorting::DateAdded => "date_added",
            Sorting::Random => "random",
            Sorting::Views => "views",
            Sorting::Favorites => "favorites",
            Sorting::Relevance => "relevance",
            Sorting::Hot => "hot",
        }
    }

    pub fn from_api(s: &str) -> Option<Sorting> {
        Sorting::ALL.into_iter().find(|v| v.api() == s)
    }

    pub fn label(self) -> &'static str {
        match self {
            Sorting::Toplist => "Top",
            Sorting::DateAdded => "Latest",
            Sorting::Random => "Random",
            Sorting::Views => "Views",
            Sorting::Favorites => "Favorites",
            Sorting::Relevance => "Relevance",
            Sorting::Hot => "Hot",
        }
    }
}

/// Toplist time ranges accepted by the API.
pub const TOP_RANGES: [&str; 7] = ["1d", "3d", "1w", "1M", "3M", "6M", "1y"];

#[derive(Debug, Clone)]
pub struct SearchParams {
    pub query: String,
    /// general, anime, people
    pub categories: [bool; 3],
    /// sfw, sketchy, nsfw
    pub purity: [bool; 3],
    pub sorting: Sorting,
    pub top_range: String,
    /// Comma-separated aspect ratios, e.g. "16x9,16x10".
    pub ratios: Option<String>,
    /// Minimum resolution, e.g. "2560x1440".
    pub atleast: Option<String>,
    /// Exact resolutions, e.g. "2560x1440,3840x2160".
    pub resolutions: Option<String>,
    /// Hex color without '#', from Wallhaven's palette.
    pub color: Option<String>,
    pub hide_ai_art: bool,
    pub page: u32,
    /// Seed returned by a previous random search, so later pages stay consistent.
    pub seed: Option<String>,
}

fn bits(flags: [bool; 3]) -> String {
    flags.iter().map(|b| if *b { '1' } else { '0' }).collect()
}

/// Account settings from /api/v1/settings, used as the default filters.
#[derive(Debug, Clone, Deserialize)]
pub struct UserSettings {
    #[serde(default)]
    pub purity: Vec<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub resolutions: Vec<String>,
    #[serde(default)]
    pub aspect_ratios: Vec<String>,
    #[serde(default)]
    pub toplist_range: String,
    #[serde(default)]
    pub ai_art_filter: serde_json::Value,
}

impl UserSettings {
    pub fn purity_flags(&self) -> [bool; 3] {
        ["sfw", "sketchy", "nsfw"].map(|p| self.purity.iter().any(|x| x == p))
    }

    pub fn category_flags(&self) -> [bool; 3] {
        ["general", "anime", "people"].map(|c| self.categories.iter().any(|x| x == c))
    }

    fn joined(list: &[String]) -> Option<String> {
        let v: Vec<&str> = list.iter().map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
        (!v.is_empty()).then(|| v.join(","))
    }

    pub fn resolutions(&self) -> Option<String> {
        Self::joined(&self.resolutions)
    }

    pub fn ratios(&self) -> Option<String> {
        Self::joined(&self.aspect_ratios)
    }

    pub fn hide_ai_art(&self) -> bool {
        match &self.ai_art_filter {
            serde_json::Value::Number(n) => n.as_i64() == Some(1),
            serde_json::Value::String(s) => s == "1",
            serde_json::Value::Bool(b) => *b,
            _ => false,
        }
    }
}

/// Extra details from /api/v1/w/{id}; search results don't carry these.
#[derive(Debug, Clone, Deserialize)]
pub struct WallpaperInfo {
    #[serde(default)]
    pub tags: Vec<Tag>,
    #[serde(default)]
    pub uploader: Option<Uploader>,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub views: u64,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Tag {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub purity: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Uploader {
    pub username: String,
    #[serde(default)]
    pub avatar: std::collections::HashMap<String, String>,
}

#[derive(Deserialize)]
struct TagResponse {
    data: Tag,
}

#[derive(Deserialize)]
struct InfoResponse {
    data: WallpaperInfo,
}

#[derive(Deserialize)]
struct SettingsResponse {
    data: UserSettings,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Wallpaper {
    pub id: String,
    pub url: String,
    pub purity: String,
    pub category: String,
    pub resolution: String,
    pub file_size: u64,
    #[serde(default)]
    pub favorites: u64,
    pub path: String,
    pub thumbs: Thumbs,
    #[serde(default)]
    pub colors: Vec<String>,
}

impl Wallpaper {
    /// File name used for the downloaded original, matching Wallhaven's own naming.
    pub fn file_name(&self) -> String {
        self.path.rsplit('/').next().map(str::to_owned).unwrap_or_else(|| format!("wallhaven-{}.jpg", self.id))
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Thumbs {
    /// Same aspect ratio as the original (the others are cropped).
    pub original: String,
    pub small: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Meta {
    pub current_page: u32,
    pub last_page: u32,
    pub total: u64,
    #[serde(default)]
    pub seed: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchResponse {
    pub data: Vec<Wallpaper>,
    pub meta: Meta,
}

/// Live state of a download, shared with the UI.
#[derive(Default)]
pub struct Progress {
    pub done: AtomicU64,
    pub total: AtomicU64,
    pub cancel: AtomicBool,
    /// Bit `i` set once byte range `i` of `chunk_count` has fully landed.
    /// In a baseline JPEG, byte order is roughly row order, so this doubles
    /// as a map of which horizontal bands of the picture have arrived.
    pub chunks: AtomicU64,
    pub chunk_count: AtomicU32,
}

impl Progress {
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// Wallhaven's CDN is fast on some connections and crawls (~150 KB/s) on
/// others, even for cache hits. Fetching many small ranges over parallel
/// connections means one slow connection only stalls a small chunk.
const DOWNLOAD_WORKERS: usize = 12;
const MIN_CHUNK: u64 = 256 * 1024;
const MAX_CHUNKS: u64 = 64;

/// Positional write (ranges land out of order), on Unix and Windows.
fn write_at(file: &fs::File, buf: &[u8], offset: u64) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt as _;
        file.write_all_at(buf, offset)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt as _;
        let mut written = 0;
        while written < buf.len() {
            written += file.seek_write(&buf[written..], offset + written as u64)?;
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct Client {
    http: reqwest::blocking::Client,
    api_key: Option<String>,
}

impl Client {
    pub fn new(api_key: &str) -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .user_agent(concat!("muralist/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(120))
            .build()?;
        let api_key = (!api_key.trim().is_empty()).then(|| api_key.trim().to_owned());
        Ok(Self { http, api_key })
    }

    pub fn has_key(&self) -> bool {
        self.api_key.is_some()
    }

    fn get(&self, url: &str) -> reqwest::blocking::RequestBuilder {
        let req = self.http.get(url);
        match &self.api_key {
            Some(key) => req.header("X-API-Key", key),
            None => req,
        }
    }

    pub fn search(&self, p: &SearchParams) -> Result<SearchResponse> {
        let mut query: Vec<(&str, String)> = vec![
            ("q", p.query.clone()),
            ("categories", bits(p.categories)),
            ("purity", bits(p.purity)),
            ("sorting", p.sorting.api().into()),
            ("order", "desc".into()),
            ("page", p.page.to_string()),
        ];
        if p.sorting == Sorting::Toplist {
            query.push(("topRange", p.top_range.clone()));
        }
        if let Some(v) = &p.atleast {
            query.push(("atleast", v.clone()));
        }
        if let Some(v) = &p.resolutions {
            query.push(("resolutions", v.clone()));
        }
        if let Some(v) = &p.ratios {
            query.push(("ratios", v.clone()));
        }
        if let Some(v) = &p.color {
            query.push(("colors", v.clone()));
        }
        query.push(("ai_art_filter", if p.hide_ai_art { "1" } else { "0" }.into()));
        if let Some(seed) = &p.seed {
            query.push(("seed", seed.clone()));
        }

        let resp = self.get(&format!("{BASE}/search")).query(&query).send().context("search request failed")?;
        match resp.status().as_u16() {
            200 => {}
            401 => bail!("Wallhaven rejected the API key (401)"),
            429 => bail!("Rate limited by Wallhaven (45 requests/min), wait a moment"),
            s => bail!("Wallhaven returned HTTP {s}"),
        }
        resp.json().context("decoding search response")
    }

    /// Name and category of a tag id (as used in `id:123` searches).
    pub fn tag(&self, id: u64) -> Result<Tag> {
        let resp = self.get(&format!("{BASE}/tag/{id}")).send()?;
        if !resp.status().is_success() {
            bail!("tag {id}: HTTP {}", resp.status());
        }
        Ok(resp.json::<TagResponse>()?.data)
    }

    pub fn info(&self, id: &str) -> Result<WallpaperInfo> {
        let resp = self.get(&format!("{BASE}/w/{id}")).send()?;
        match resp.status().as_u16() {
            200 => {}
            429 => bail!("rate limited, try again in a moment"),
            s => bail!("HTTP {s}"),
        }
        Ok(resp.json::<InfoResponse>()?.data)
    }

    pub fn settings(&self) -> Result<UserSettings> {
        if self.api_key.is_none() {
            bail!("no API key");
        }
        let resp = self.get(&format!("{BASE}/settings")).send()?;
        if !resp.status().is_success() {
            bail!("settings: HTTP {}", resp.status());
        }
        Ok(resp.json::<SettingsResponse>()?.data)
    }

    /// Like [`Self::download`], but splits large files into byte ranges fetched
    /// over parallel connections, reporting into `progress`.
    pub fn download_parallel(&self, url: &str, dest: &Path, progress: &Progress) -> Result<PathBuf> {
        self.download_parallel_with(url, dest, progress, DOWNLOAD_WORKERS)
    }

    /// [`Self::download_parallel`] with an explicit connection count.
    pub fn download_parallel_with(
        &self,
        url: &str,
        dest: &Path,
        progress: &Progress,
        workers: usize,
    ) -> Result<PathBuf> {
        if let Ok(meta) = fs::metadata(dest) {
            progress.total.store(meta.len(), Ordering::Relaxed);
            progress.done.store(meta.len(), Ordering::Relaxed);
            return Ok(dest.to_owned());
        }
        if let Some(dir) = dest.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = dest.with_extension("part");

        let head = self.http.head(url).send().with_context(|| format!("HEAD {url}"))?;
        let size = head.headers().get(CONTENT_LENGTH).and_then(|v| v.to_str().ok()?.parse::<u64>().ok());
        let ranges = head.headers().get(ACCEPT_RANGES).is_some_and(|v| v.as_bytes() == b"bytes");

        match size {
            Some(size) if ranges && size >= MIN_CHUNK * 2 => {
                progress.total.store(size, Ordering::Relaxed);
                let file = fs::File::create(&tmp)?;
                file.set_len(size)?;
                let chunk = size.div_ceil(MAX_CHUNKS).max(MIN_CHUNK);
                let chunks = size.div_ceil(chunk) as usize;
                progress.chunk_count.store(chunks as u32, Ordering::Relaxed);
                let next = AtomicUsize::new(0);
                let result = std::thread::scope(|scope| {
                    let workers: Vec<_> = (0..workers.max(1).min(chunks))
                        .map(|_| {
                            scope.spawn(|| -> Result<()> {
                                loop {
                                    let ix = next.fetch_add(1, Ordering::Relaxed);
                                    if ix >= chunks {
                                        return Ok(());
                                    }
                                    let start = ix as u64 * chunk;
                                    let end = (start + chunk).min(size) - 1;
                                    if let Err(e) = self.fetch_range(url, &file, start, end, progress) {
                                        // Stop the other workers' chunks too.
                                        progress.cancel.store(true, Ordering::Relaxed);
                                        return Err(e);
                                    }
                                    progress.chunks.fetch_or(1 << ix, Ordering::Relaxed);
                                }
                            })
                        })
                        .collect();
                    workers
                        .into_iter()
                        .map(|w| w.join().map_err(|_| anyhow!("download worker panicked"))?)
                        .collect::<Result<Vec<()>>>()
                });
                if let Err(e) = result {
                    let _ = fs::remove_file(&tmp);
                    return Err(e);
                }
            }
            _ => {
                if let Some(size) = size {
                    progress.total.store(size, Ordering::Relaxed);
                }
                let mut resp = self.get(url).send().with_context(|| format!("GET {url}"))?;
                if !resp.status().is_success() {
                    bail!("GET {url}: HTTP {}", resp.status());
                }
                let mut file = fs::File::create(&tmp)?;
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    if progress.cancelled() {
                        let _ = fs::remove_file(&tmp);
                        bail!("cancelled");
                    }
                    let n = resp.read(&mut buf)?;
                    if n == 0 {
                        break;
                    }
                    file.write_all(&buf[..n])?;
                    progress.done.fetch_add(n as u64, Ordering::Relaxed);
                }
            }
        }
        fs::rename(&tmp, dest)?;
        Ok(dest.to_owned())
    }

    /// Fetch bytes `start..=end` into `file`, resuming up to twice on errors.
    fn fetch_range(&self, url: &str, file: &fs::File, start: u64, end: u64, progress: &Progress) -> Result<()> {
        let mut pos = start;
        let mut attempt = 0;
        loop {
            let result = (|| -> Result<()> {
                let mut resp = self.http.get(url).header(RANGE, format!("bytes={pos}-{end}")).send()?;
                if resp.status() != StatusCode::PARTIAL_CONTENT {
                    bail!("server ignored range request (HTTP {})", resp.status());
                }
                let mut buf = vec![0u8; 64 * 1024];
                while pos <= end {
                    if progress.cancelled() {
                        bail!("cancelled");
                    }
                    let n = resp.read(&mut buf)?;
                    if n == 0 {
                        bail!("connection closed early");
                    }
                    let n = n.min((end + 1 - pos) as usize);
                    write_at(file, &buf[..n], pos)?;
                    pos += n as u64;
                    progress.done.fetch_add(n as u64, Ordering::Relaxed);
                }
                Ok(())
            })();
            match result {
                Ok(()) => return Ok(()),
                Err(_) if attempt < 2 && !progress.cancelled() => attempt += 1,
                Err(e) => return Err(e),
            }
        }
    }

    /// Download `url` to `dest` unless it already exists. Writes to a temp file
    /// first so a failed download never leaves a truncated image behind.
    pub fn download(&self, url: &str, dest: &Path) -> Result<PathBuf> {
        if dest.exists() {
            return Ok(dest.to_owned());
        }
        if let Some(dir) = dest.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut resp = self.get(url).send().with_context(|| format!("GET {url}"))?;
        if !resp.status().is_success() {
            bail!("GET {url}: HTTP {}", resp.status());
        }
        let tmp = dest.with_extension("part");
        let mut file = fs::File::create(&tmp)?;
        resp.copy_to(&mut file)?;
        file.flush()?;
        drop(file);
        fs::rename(&tmp, dest)?;
        Ok(dest.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compares parallel ranged vs single-stream downloads of fresh uploads
    /// (likely uncached at the CDN): `cargo test bench -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn bench_parallel_download() {
        let client = Client::new("").unwrap();
        let resp = client
            .search(&SearchParams {
                query: String::new(),
                categories: [true, true, true],
                purity: [true, false, false],
                sorting: Sorting::DateAdded,
                top_range: "1M".into(),
                ratios: None,
                atleast: Some("2560x1440".into()),
                resolutions: None,
                color: None,
                hide_ai_art: false,
                page: 9,
                seed: None,
            })
            .unwrap();
        let dir = std::env::temp_dir().join(format!("muralist-bench-{}", std::process::id()));
        for (i, w) in resp.data.iter().take(6).enumerate() {
            let dest = dir.join(w.file_name());
            let t = std::time::Instant::now();
            let parallel = i < 5;
            let mode = if parallel { "parallel" } else { "single  " };
            if parallel {
                client.download_parallel(&w.path, &dest, &Progress::default()).unwrap();
            } else {
                client.download(&w.path, &dest).unwrap();
            }
            let secs = t.elapsed().as_secs_f64();
            let len = fs::metadata(&dest).unwrap().len();
            assert_eq!(len, w.file_size, "size mismatch for {}", w.id);
            image::image_dimensions(&dest).expect("downloaded file decodes");
            println!("{mode} {:5.1} MB  {secs:5.2}s  {:6.2} MB/s", len as f64 / 1e6, len as f64 / secs / 1e6);
        }
        fs::remove_dir_all(&dir).ok();
    }

    /// Hits the real API: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn search_download_and_span() {
        let client = Client::new(&std::env::var("WALLHAVEN_API_KEY").unwrap_or_default()).unwrap();
        let resp = client
            .search(&SearchParams {
                query: String::new(),
                categories: [true, false, false],
                purity: [true, false, false],
                sorting: Sorting::Toplist,
                top_range: "1M".into(),
                ratios: Some("32x9,48x9".into()),
                atleast: None,
                resolutions: None,
                color: None,
                hide_ai_art: true,
                page: 1,
                seed: None,
            })
            .unwrap();
        let w = resp.data.first().expect("at least one ultrawide result");
        let dir = std::env::temp_dir().join(format!("muralist-test-{}", std::process::id()));
        let full = client.download(&w.path, &dir.join(w.file_name())).unwrap();
        assert!(image::image_dimensions(&full).unwrap().0 >= 3840);

        let monitors: Vec<_> = (0..3)
            .map(|i| crate::monitors::Monitor {
                name: format!("DP-{}", i + 1),
                x: i * 2560,
                y: 0,
                width: 2560,
                height: 1440,
                px_width: 2560,
                px_height: 1440,
                primary: i == 1,
            })
            .collect();
        let slices = crate::span::slice(&full, &monitors, &dir.join("span")).unwrap();
        assert_eq!(slices.len(), 3);
        for path in slices.values() {
            assert_eq!(image::image_dimensions(path).unwrap(), (2560, 1440));
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
