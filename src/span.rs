//! Slice one wide image across several monitors.

use std::collections::BTreeMap;
use std::fs;
use std::io::BufWriter;
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

/// How the image sits behind the monitors: `zoom` >= 1 beyond the minimum
/// cover scale, `pan` in -1..1 per axis across the spare image (0 = centered).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Framing {
    pub zoom: f32,
    pub pan: (f32, f32),
}

impl Default for Framing {
    fn default() -> Self {
        Self { zoom: 1., pan: (0., 0.) }
    }
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
    let img = image::open(src).with_context(|| format!("opening {}", src.display()))?;
    fs::create_dir_all(out_dir)?;
    let stem = src.file_stem().map_or_else(|| "span".into(), |s| s.to_string_lossy().into_owned());

    let mut result = BTreeMap::new();
    // Distinct names per framing: desktops (Plasma, for one) won't reload an image whose path
    // it already shows.
    let tag = if framing == Framing::default() {
        String::new()
    } else {
        format!("-z{:.0}x{:.0}y{:.0}", framing.zoom * 100., framing.pan.0 * 100., framing.pan.1 * 100.)
    };
    for (m, c) in monitors.iter().zip(crops_framed(img.width(), img.height(), monitors, framing)) {
        let part =
            img.crop_imm(c.x, c.y, c.w, c.h).resize_exact(m.px_width, m.px_height, FilterType::Lanczos3).to_rgb8();
        let dest = out_dir.join(format!("{stem}-span{tag}-{}.jpg", m.name));
        let file = fs::File::create(&dest)?;
        JpegEncoder::new_with_quality(BufWriter::new(file), 94)
            .encode_image(&part)
            .with_context(|| format!("writing {}", dest.display()))?;
        result.insert(m.name.clone(), dest);
    }
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
        let top = crops_framed(7680, 2880, &ms, Framing { zoom: 1., pan: (0., -1.) });
        let bottom = crops_framed(7680, 2880, &ms, Framing { zoom: 1., pan: (0., 1.) });
        assert_eq!(top[0].y, 0);
        assert_eq!(bottom[0].y + bottom[0].h, 2880);
    }

    #[test]
    fn zoom_shrinks_the_source_region() {
        let ms = [mon("DP-1", 0), mon("DP-2", 2560), mon("DP-3", 5120)];
        let plain = crops_framed(7680, 1440, &ms, Framing::default());
        let zoomed = crops_framed(7680, 1440, &ms, Framing { zoom: 2., pan: (0., 0.) });
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
}
