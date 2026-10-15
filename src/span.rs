//! Slice one wide image across several monitors.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;

use crate::monitors::Monitor;

/// Pixel rectangle inside the source image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Crop {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Which way up the picture goes: quarter turns clockwise, then mirrored
/// left to right. Together they reach all eight orientations (an upside-
/// down flip is a half turn mirrored).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Orient {
    pub turns: u8,
    pub mirror: bool,
}

impl Orient {
    pub fn rotated_cw(self) -> Self {
        Self { turns: (self.turns + 1) % 4, ..self }
    }

    pub fn rotated_ccw(self) -> Self {
        Self { turns: (self.turns + 3) % 4, ..self }
    }

    /// Mirrored left to right, as seen.
    pub fn mirrored(self) -> Self {
        Self { mirror: !self.mirror, ..self }
    }

    /// Flipped upside down, as seen.
    pub fn flipped(self) -> Self {
        Self { turns: (self.turns + 2) % 4, mirror: !self.mirror }
    }

    /// The picture's size once turned.
    pub fn dims(self, (w, h): (u32, u32)) -> (u32, u32) {
        if self.turns % 2 == 1 { (h, w) } else { (w, h) }
    }

    pub fn apply(self, img: image::DynamicImage) -> image::DynamicImage {
        let img = match self.turns % 4 {
            1 => img.rotate90(),
            2 => img.rotate180(),
            3 => img.rotate270(),
            _ => img,
        };
        if self.mirror { img.fliph() } else { img }
    }
}

/// How the image sits behind the monitors: `zoom` >= 1 beyond the minimum
/// cover scale, `pan` in -1..1 per axis across the spare image (0 = centered),
/// and which way up it goes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Framing {
    pub zoom: f32,
    pub pan: (f32, f32),
    pub orient: Orient,
}

impl Default for Framing {
    fn default() -> Self {
        Self { zoom: 1., pan: (0., 0.), orient: Orient::default() }
    }
}

impl Framing {
    /// A file-name tag naming this framing (empty for the default), since
    /// desktops won't reload an image whose path they already show.
    fn tag(self) -> String {
        if self == Framing::default() {
            return String::new();
        }
        let o = self.orient;
        format!(
            "-z{:.0}x{:.0}y{:.0}r{}{}",
            self.zoom * 100.,
            self.pan.0 * 100.,
            self.pan.1 * 100.,
            o.turns,
            if o.mirror { "m" } else { "" }
        )
    }
}

/// An output name kept to safe characters for a file name.
fn safe_name(name: &str) -> String {
    name.chars().map(|ch| if ch.is_ascii_alphanumeric() || ch == '-' { ch } else { '_' }).collect()
}

/// Cuts `crop` out of `img`, sizes it for `m` and writes it to `dest`.
fn write_part(img: &image::DynamicImage, crop: Crop, m: &Monitor, dest: &Path) -> Result<()> {
    let part = img
        .crop_imm(crop.x, crop.y, crop.w, crop.h)
        .resize_exact(m.px_width, m.px_height, FilterType::Lanczos3)
        .to_rgb8();
    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(&mut bytes, 94).encode_image(&part)?;
    crate::store::write_atomic(dest, &bytes).with_context(|| format!("writing {}", dest.display()))
}

/// Each monitor framed on its own: its own zoom, pan and orientation of the
/// same picture, each covering just that screen. Monitors without a framing
/// get the default. Returns output name -> file.
pub fn frame_each(
    src: &Path,
    monitors: &[Monitor],
    framings: &BTreeMap<String, Framing>,
    out_dir: &Path,
) -> Result<BTreeMap<String, PathBuf>> {
    if monitors.is_empty() {
        bail!("no monitors to frame");
    }
    let img = image::open(src).with_context(|| format!("opening {}", src.display()))?;
    let stem = src.file_stem().map_or_else(|| "frame".into(), |s| s.to_string_lossy().into_owned());
    let parts: Vec<Result<(String, PathBuf)>> = std::thread::scope(|scope| {
        let workers: Vec<_> = monitors
            .iter()
            .map(|m| {
                let (img, stem) = (&img, &stem);
                let framing = framings.get(&m.name).copied().unwrap_or_default();
                scope.spawn(move || -> Result<(String, PathBuf)> {
                    let turned = framing.orient.apply(img.clone());
                    let crop = crops_framed(turned.width(), turned.height(), std::slice::from_ref(m), framing)[0];
                    let dest = out_dir.join(format!("{stem}-frame{}-{}.jpg", framing.tag(), safe_name(&m.name)));
                    write_part(&turned, crop, m, &dest)?;
                    Ok((m.name.clone(), dest))
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|w| w.join().unwrap_or_else(|_| Err(anyhow::anyhow!("framing worker panicked"))))
            .collect()
    });
    parts.into_iter().collect()
}

/// Geometry of a framing in desktop (logical) units: image scale (units per
/// image pixel) and how far the image's top-left sits up and left of the
/// monitors' bounding box.
pub fn layout(img_w: u32, img_h: u32, bbox_w: f64, bbox_h: f64, framing: Framing) -> (f64, f64, f64) {
    let (iw, ih) = (f64::from(img_w), f64::from(img_h));
    let s = (bbox_w / iw).max(bbox_h / ih) * f64::from(framing.zoom.max(1.));
    let slack_x = (iw * s - bbox_w).max(0.);
    let slack_y = (ih * s - bbox_h).max(0.);
    let off_x = slack_x / 2. * (1. + f64::from(framing.pan.0.clamp(-1., 1.)));
    let off_y = slack_y / 2. * (1. + f64::from(framing.pan.1.clamp(-1., 1.)));
    (s, off_x, off_y)
}

/// Scale the image to cover the bounding box of all monitors (centered, aspect
/// preserved) and return each monitor's region in source-image pixels.
#[cfg(test)]
pub fn crops(img_w: u32, img_h: u32, monitors: &[Monitor]) -> Vec<Crop> {
    crops_framed(img_w, img_h, monitors, Framing::default())
}

/// [`crops`] with an explicit zoom and pan.
pub fn crops_framed(img_w: u32, img_h: u32, monitors: &[Monitor], framing: Framing) -> Vec<Crop> {
    let min_x = f64::from(monitors.iter().map(|m| m.x).min().unwrap_or(0));
    let min_y = f64::from(monitors.iter().map(|m| m.y).min().unwrap_or(0));
    let max_x = f64::from(monitors.iter().map(|m| m.x + m.width as i32).max().unwrap_or(0));
    let max_y = f64::from(monitors.iter().map(|m| m.y + m.height as i32).max().unwrap_or(0));
    let (bw, bh) = (max_x - min_x, max_y - min_y);
    let (iw, ih) = (f64::from(img_w), f64::from(img_h));

    // Logical units per image pixel, and the image overhang up/left.
    let (s, off_x, off_y) = layout(img_w, img_h, bw, bh, framing);

    monitors
        .iter()
        .map(|m| {
            let x0 = ((f64::from(m.x) - min_x + off_x) / s).round().clamp(0.0, iw - 1.0);
            let y0 = ((f64::from(m.y) - min_y + off_y) / s).round().clamp(0.0, ih - 1.0);
            let w = (f64::from(m.width) / s).round().clamp(1.0, iw - x0);
            let h = (f64::from(m.height) / s).round().clamp(1.0, ih - y0);
            Crop { x: x0 as u32, y: y0 as u32, w: w as u32, h: h as u32 }
        })
        .collect()
}

/// Write one JPEG per monitor into `out_dir`, returning output name -> file.
pub fn slice(src: &Path, monitors: &[Monitor], out_dir: &Path) -> Result<BTreeMap<String, PathBuf>> {
    slice_framed(src, monitors, out_dir, Framing::default())
}

/// [`slice`] with an explicit zoom and pan.
pub fn slice_framed(
    src: &Path,
    monitors: &[Monitor],
    out_dir: &Path,
    framing: Framing,
) -> Result<BTreeMap<String, PathBuf>> {
    if monitors.is_empty() {
        bail!("no monitors to span across");
    }
    let img = framing.orient.apply(image::open(src).with_context(|| format!("opening {}", src.display()))?);
    fs::create_dir_all(out_dir)?;
    let stem = src.file_stem().map_or_else(|| "span".into(), |s| s.to_string_lossy().into_owned());
    let tag = framing.tag();
    // Each slice is a big resize and encode of its own: one thread each.
    let crops = crops_framed(img.width(), img.height(), monitors, framing);
    let slices: Vec<Result<(String, PathBuf)>> = std::thread::scope(|scope| {
        let workers: Vec<_> = monitors
            .iter()
            .zip(crops)
            .map(|(m, c)| {
                let (img, stem, tag) = (&img, &stem, &tag);
                scope.spawn(move || -> Result<(String, PathBuf)> {
                    let dest = out_dir.join(format!("{stem}-span{tag}-{}.jpg", safe_name(&m.name)));
                    write_part(img, c, m, &dest)?;
                    Ok((m.name.clone(), dest))
                })
            })
            .collect();
        workers.into_iter().map(|w| w.join().unwrap_or_else(|_| Err(anyhow::anyhow!("span worker panicked")))).collect()
    });
    let result = slices.into_iter().collect::<Result<BTreeMap<_, _>>>()?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mon(name: &str, x: i32) -> Monitor {
        Monitor {
            name: name.into(),
            x,
            y: 0,
            width: 2560,
            height: 1440,
            px_width: 2560,
            px_height: 1440,
            primary: false,
        }
    }

    #[test]
    fn exact_fit_splits_evenly() {
        let ms = [mon("DP-1", 0), mon("DP-2", 2560), mon("DP-3", 5120)];
        let c = crops(7680, 1440, &ms);
        assert_eq!(c[0], Crop { x: 0, y: 0, w: 2560, h: 1440 });
        assert_eq!(c[1], Crop { x: 2560, y: 0, w: 2560, h: 1440 });
        assert_eq!(c[2], Crop { x: 5120, y: 0, w: 2560, h: 1440 });
    }

    #[test]
    fn pan_slides_the_window_to_the_edges() {
        let ms = [mon("DP-1", 0), mon("DP-2", 2560), mon("DP-3", 5120)];
        // 7680x2880 is twice as tall as needed: pan -1 takes the top, +1 the bottom.
        let top = crops_framed(7680, 2880, &ms, Framing { zoom: 1., pan: (0., -1.), ..Framing::default() });
        let bottom = crops_framed(7680, 2880, &ms, Framing { zoom: 1., pan: (0., 1.), ..Framing::default() });
        assert_eq!(top[0].y, 0);
        assert_eq!(bottom[0].y + bottom[0].h, 2880);
    }

    #[test]
    fn zoom_shrinks_the_source_region() {
        let ms = [mon("DP-1", 0), mon("DP-2", 2560), mon("DP-3", 5120)];
        let plain = crops_framed(7680, 1440, &ms, Framing::default());
        let zoomed = crops_framed(7680, 1440, &ms, Framing { zoom: 2., pan: (0., 0.), ..Framing::default() });
        assert_eq!(zoomed[1].w * 2, plain[1].w);
    }

    #[test]
    fn narrow_image_is_cropped_vertically_and_centered() {
        // 32:9 image on a 48:9 desktop: width-bound, top and bottom cut off.
        let ms = [mon("DP-1", 0), mon("DP-2", 2560), mon("DP-3", 5120)];
        let c = crops(5120, 1440, &ms);
        let s = 7680.0 / 5120.0;
        let h = (1440.0 / s) as u32;
        assert_eq!(c[0].h, h);
        assert_eq!(c[0].y, ((1440.0 * s - 1440.0) / 2.0 / s) as u32);
        assert_eq!(c[0].x + c[0].w, c[1].x);
        assert_eq!(c[2].x + c[2].w, 5120);
    }

    #[test]
    fn each_screen_is_framed_and_turned_on_its_own() {
        let dir = std::env::temp_dir().join(format!("muralist-frame-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        // Left half red, right half blue.
        let src = dir.join("src.png");
        image::RgbImage::from_fn(
            400,
            200,
            |x, _| if x < 200 { image::Rgb([255, 0, 0]) } else { image::Rgb([0, 0, 255]) },
        )
        .save(&src)
        .unwrap();
        let mut a = mon("A", 0);
        let mut b = mon("B", 100);
        for m in [&mut a, &mut b] {
            (m.width, m.height, m.px_width, m.px_height) = (100, 100, 20, 20);
        }
        let left = Framing { pan: (-1., 0.), ..Framing::default() };
        let mirrored = Framing { orient: Orient::default().mirrored(), ..left };
        let framings = BTreeMap::from([("A".to_owned(), left), ("B".to_owned(), mirrored)]);
        let out = frame_each(&src, &[a, b], &framings, &dir).unwrap();
        let pixel = |name: &str| image::open(&out[name]).unwrap().to_rgb8().get_pixel(10, 10).0;
        assert!(pixel("A")[0] > 200 && pixel("A")[2] < 60, "the left edge is red");
        assert!(pixel("B")[2] > 200 && pixel("B")[0] < 60, "mirrored, the left edge is blue");
        let _ = fs::remove_dir_all(&dir);
    }
}
