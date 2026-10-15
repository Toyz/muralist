//! The interface takes on the selected wallpaper's colors: accent from its
//! most vivid tone, surfaces tinted by its dominant hue, eased over ~600ms.

use std::cell::Cell;
use std::path::Path;
use std::time::Instant;

use gpui_kit::*;

use super::{Pal, hex};

const TRANSITION_SECS: f32 = 0.6;

thread_local! {
    static CURRENT: Cell<Pal> = Cell::new(base_pal());
}

pub(super) fn base_pal() -> Pal {
    readable_text(raw_base())
}

fn raw_base() -> Pal {
    Pal {
        bg: hex(0x0b0d12),
        surface: hex(0x10131a),
        surface2: hex(0x161a23),
        elevated: hex(0x1d2230),
        border: hex(0x222736),
        border_strong: hex(0x323950),
        fg: hex(0xe7e9f0),
        muted: hex(0xa3a9b9),
        dim: hex(0x737a8e),
        accent: hex(0x8b5cf6),
        accent_hover: hex(0x9d75f8),
        ok: hex(0x22c55e),
        error: hex(0xef4444),
    }
}

pub(super) fn current() -> Pal {
    CURRENT.with(std::cell::Cell::get)
}

pub(super) fn set_current(p: Pal) {
    CURRENT.with(|c| c.set(p));
}

/// WCAG relative luminance.
fn luminance(c: Hsla) -> f32 {
    let c: Rgba = c.into();
    let lin = |v: f32| if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
    0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
}

/// WCAG contrast ratio between two colors (1..21).
pub(super) fn contrast(a: Hsla, b: Hsla) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Lighten `fg` (dark theme) until it reaches `ratio` against `bg`.
pub(super) fn readable(fg: Hsla, bg: Hsla, ratio: f32) -> Hsla {
    let mut c = fg;
    while contrast(c, bg) < ratio && c.l < 1. {
        c.l = (c.l + 0.015).min(1.);
        // Very light colors read better a little less saturated.
        if c.l > 0.8 {
            c.s *= 0.97;
        }
    }
    c
}

/// Make every text color pass contrast against the lightest surface text
/// can sit on (cards, chips, glass), whatever the palette is.
fn readable_text(mut p: Pal) -> Pal {
    let worst = p.elevated;
    let card = p.surface2;
    p.fg = readable(p.fg, worst, 12.);
    p.muted = readable(p.muted, worst, 6.);
    p.dim = readable(p.dim, worst, 4.5);
    p.accent = readable(p.accent, card, 4.5);
    p.accent_hover = readable(p.accent_hover, card, 5.5);
    p.ok = readable(p.ok, card, 4.5);
    p.error = readable(p.error, card, 4.5);
    p
}

/// Palette for an image whose colors are `colors`, dominant first.
pub(super) fn derive(colors: &[Hsla]) -> Pal {
    let base = raw_base();
    let Some(&dominant) = colors.first() else {
        return readable_text(base);
    };
    let mut p = base;

    // Accent: the most vivid mid-lightness color, normalized so it stays
    // readable on the dark surfaces.
    let vividness = |c: &Hsla| c.s * (1. - (c.l - 0.5).abs() * 1.6).max(0.);
    if let Some(a) = colors
        .iter()
        .filter(|c| c.s > 0.22 && c.l > 0.12 && c.l < 0.92)
        .max_by(|a, b| vividness(a).total_cmp(&vividness(b)))
    {
        p.accent = Hsla { h: a.h, s: a.s.clamp(0.55, 0.85), l: 0.64, a: 1. };
        p.accent_hover = Hsla { l: 0.72, ..p.accent };
    }

    // Surfaces: a dark wash of the dominant hue (grays keep the stock blue).
    let (h, s) = if dominant.s > 0.08 { (dominant.h, (dominant.s * 0.35).clamp(0.08, 0.28)) } else { (0.625, 0.22) };
    let tone = |l: f32| Hsla { h, s, l, a: 1. };
    p.bg = tone(0.055);
    p.surface = tone(0.075);
    p.surface2 = tone(0.10);
    p.elevated = tone(0.135);
    p.border = tone(0.155);
    p.border_strong = tone(0.22);
    // Secondary text takes a whisper of the wallpaper's hue; contrast is
    // then enforced on top.
    let text_tint = |c: Hsla| Hsla { h, s: (s * 0.6).min(0.14), ..c };
    p.muted = text_tint(p.muted);
    p.dim = text_tint(p.dim);
    readable_text(p)
}

fn lerp_hsla(a: Hsla, b: Hsla, t: f32) -> Hsla {
    // Shortest way around the hue circle; unsaturated ends barely show hue.
    let mut dh = b.h - a.h;
    if dh > 0.5 {
        dh -= 1.;
    } else if dh < -0.5 {
        dh += 1.;
    }
    Hsla {
        h: (a.h + dh * t).rem_euclid(1.),
        s: a.s + (b.s - a.s) * t,
        l: a.l + (b.l - a.l) * t,
        a: a.a + (b.a - a.a) * t,
    }
}

fn lerp(a: &Pal, b: &Pal, t: f32) -> Pal {
    Pal {
        bg: lerp_hsla(a.bg, b.bg, t),
        surface: lerp_hsla(a.surface, b.surface, t),
        surface2: lerp_hsla(a.surface2, b.surface2, t),
        elevated: lerp_hsla(a.elevated, b.elevated, t),
        border: lerp_hsla(a.border, b.border, t),
        border_strong: lerp_hsla(a.border_strong, b.border_strong, t),
        fg: lerp_hsla(a.fg, b.fg, t),
        muted: lerp_hsla(a.muted, b.muted, t),
        dim: lerp_hsla(a.dim, b.dim, t),
        accent: lerp_hsla(a.accent, b.accent, t),
        accent_hover: lerp_hsla(a.accent_hover, b.accent_hover, t),
        ok: lerp_hsla(a.ok, b.ok, t),
        error: lerp_hsla(a.error, b.error, t),
    }
}

/// Eases the live palette toward whatever the selection asks for.
pub(super) struct ThemeAnim {
    from: Pal,
    to: Pal,
    start: Instant,
}

impl Default for ThemeAnim {
    fn default() -> Self {
        Self { from: base_pal(), to: base_pal(), start: Instant::now() }
    }
}

impl ThemeAnim {
    /// The palette for this frame, and whether to keep animating.
    pub(super) fn step(&mut self, target: Pal) -> (Pal, bool) {
        let now = Instant::now();
        let t = ((now - self.start).as_secs_f32() / TRANSITION_SECS).min(1.);
        let eased = super::glass::ease_out_cubic(t);
        let cur = lerp(&self.from, &self.to, eased);
        if target != self.to {
            self.from = cur;
            self.to = target;
            self.start = now;
            return (cur, true);
        }
        (cur, t < 1.)
    }
}

/// Dominant-first colors of a local image: k-means over its thumbnail's
/// pixels, clusters ordered by how much of the picture they cover.
pub(super) fn image_palette(path: &Path) -> Vec<Hsla> {
    let Ok(img) = image::open(path) else {
        return Vec::new();
    };
    let small = img.thumbnail(40, 40).to_rgb8();
    let pts: Vec<[f32; 3]> = small.pixels().map(|p| p.0.map(|v| f32::from(v) / 255.)).collect();
    kmeans(&pts, 5).into_iter().map(|[r, g, b]| Rgba { r, g, b, a: 1. }.into()).collect()
}

/// Plain k-means with farthest-point seeding (deterministic). Returns
/// centers sorted by cluster size, largest first; empty clusters dropped.
pub(super) fn kmeans(pts: &[[f32; 3]], k: usize) -> Vec<[f32; 3]> {
    kmeans_weighted(pts, k).into_iter().map(|(c, _)| c).collect()
}

/// Like [`kmeans`], with each center's share of the points (0..1).
pub(super) fn kmeans_weighted(pts: &[[f32; 3]], k: usize) -> Vec<([f32; 3], f32)> {
    if pts.is_empty() {
        return Vec::new();
    }
    let dist = |a: &[f32; 3], b: &[f32; 3]| (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2);
    // Seed with the mean, then repeatedly the point farthest from all seeds.
    let n = pts.len() as f32;
    let mean = pts.iter().fold([0.; 3], |m, p| [m[0] + p[0] / n, m[1] + p[1] / n, m[2] + p[2] / n]);
    let mut centers = vec![mean];
    while centers.len() < k {
        let far = pts
            .iter()
            .max_by(|a, b| {
                let da = centers.iter().map(|c| dist(a, c)).fold(f32::MAX, f32::min);
                let db = centers.iter().map(|c| dist(b, c)).fold(f32::MAX, f32::min);
                da.total_cmp(&db)
            })
            .copied()
            .unwrap();
        centers.push(far);
    }
    let mut counts = vec![0usize; k];
    for _ in 0..10 {
        let mut sums = vec![[0f32; 3]; k];
        counts = vec![0; k];
        for p in pts {
            let (ix, _) =
                centers.iter().enumerate().map(|(i, c)| (i, dist(p, c))).min_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
            counts[ix] += 1;
            for d in 0..3 {
                sums[ix][d] += p[d];
            }
        }
        for i in 0..k {
            if counts[i] > 0 {
                let c = counts[i] as f32;
                centers[i] = [sums[i][0] / c, sums[i][1] / c, sums[i][2] / c];
            }
        }
    }
    let total = pts.len() as f32;
    let mut ranked: Vec<(usize, [f32; 3])> = counts.into_iter().zip(centers).filter(|(n, _)| *n > 0).collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0));
    ranked.into_iter().map(|(n, c)| (c, n as f32 / total)).collect()
}

/// An image's palette with how much of it each color covers, biggest first.
pub(super) fn weighted_palette(path: &Path) -> Vec<([f32; 3], f32)> {
    let Ok(img) = image::open(path) else {
        return Vec::new();
    };
    let small = img.thumbnail(48, 48).to_rgb8();
    let pts: Vec<[f32; 3]> = small.pixels().map(|p| p.0.map(|v| f32::from(v) / 255.)).collect();
    kmeans_weighted(&pts, 6)
}

#[cfg(test)]
mod tests {
    use super::{contrast, derive, kmeans};
    use gpui_kit::hsla;

    #[test]
    fn text_stays_readable_on_any_palette() {
        // Bright, saturated, and pale palettes alike.
        for colors in [
            vec![hsla(0.15, 1.0, 0.5, 1.), hsla(0.1, 0.9, 0.7, 1.)],
            vec![hsla(0.0, 0.0, 0.97, 1.)],
            vec![hsla(0.62, 0.9, 0.3, 1.), hsla(0.95, 1.0, 0.6, 1.)],
            vec![],
        ] {
            let p = derive(&colors);
            assert!(contrast(p.fg, p.elevated) >= 12.);
            assert!(contrast(p.muted, p.elevated) >= 6.);
            assert!(contrast(p.dim, p.elevated) >= 4.5);
            assert!(contrast(p.accent, p.surface2) >= 4.5);
        }
    }

    #[test]
    fn kmeans_finds_the_two_blobs_biggest_first() {
        let mut pts = vec![[0.9, 0.1, 0.1]; 30];
        pts.extend(vec![[0.1, 0.1, 0.9]; 10]);
        let c = kmeans(&pts, 2);
        assert_eq!(c.len(), 2);
        assert!(c[0][0] > 0.8 && c[1][2] > 0.8);
    }
}
