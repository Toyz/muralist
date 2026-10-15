//! Grid flavors: a justified "mosaic" that keeps every wallpaper's real
//! shape, hue ("rainbow") ordering, and holographic glare on hover.

use gpui_kit::*;

use super::{CARD_INNER_R, Item, parse_hex, radius};

/// Aspect ratios outside this range are clamped so one extreme image can't
/// eat or starve a whole row.
const MIN_ASPECT: f32 = 0.45;
const MAX_ASPECT: f32 = 3.6;

/// Pack items into rows of equal height that exactly fill `width`, like a
/// photo library. Returns, per row, `(item index, card width)`.
pub(super) fn justify(aspects: &[f32], width: f32, row_h: f32, gap: f32) -> Vec<Vec<(usize, f32)>> {
    let mut rows = Vec::new();
    let mut row: Vec<(usize, f32)> = Vec::new();
    let mut natural = 0.;
    for (ix, aspect) in aspects.iter().enumerate() {
        let w = aspect.clamp(MIN_ASPECT, MAX_ASPECT) * row_h;
        row.push((ix, w));
        natural += w;
        let gaps = gap * (row.len() - 1) as f32;
        if natural + gaps >= width {
            // Stretch or shrink this row's cards to land exactly on the edge.
            let scale = (width - gaps) / natural;
            rows.push(row.drain(..).map(|(ix, w)| (ix, (w * scale).floor())).collect());
            natural = 0.;
        }
    }
    if !row.is_empty() {
        // The last row keeps its natural widths rather than ballooning.
        rows.push(row);
    }
    rows
}

/// Sort key that lays saturated colors out around the hue wheel, with the
/// near-grays trailing from dark to light.
pub(super) fn hue_key(colors: &[Hsla]) -> (u8, f32) {
    match colors.first() {
        Some(c) if c.s >= 0.12 && c.l > 0.06 => (0, c.h),
        Some(c) => (1, c.l),
        None => (2, 0.),
    }
}

pub(super) fn item_colors(item: &Item, local: &std::collections::HashMap<std::path::PathBuf, Vec<Hsla>>) -> Vec<Hsla> {
    match item {
        Item::Remote(w) => w.colors.iter().filter_map(|c| parse_hex(c)).collect(),
        Item::Local(path) => local.get(path).cloned().unwrap_or_default(),
    }
}

/// Glare and sheen that follow the pointer at `(rx, ry)` (0..1 across the
/// card), in the image's own colors: the glare takes its lightest tone, the
/// sheen sweeps between its two most vivid ones.
pub(super) fn holo_layers(rx: f32, ry: f32, colors: &[Hsla]) -> [AnyElement; 2] {
    let (vx, vy) = (0.5 - rx, 0.5 - ry);
    let angle = vx.atan2(-vy).to_degrees();

    let light = colors
        .iter()
        .copied()
        .max_by(|a, b| a.l.total_cmp(&b.l))
        .map(|c| Hsla { l: c.l.max(0.85), s: c.s.min(0.6), ..c })
        .unwrap_or(hsla(0., 0., 1., 1.));
    let glare = radius(
        div().absolute().top_0().left_0().size_full().bg(linear_gradient(
            angle,
            linear_color_stop(Hsla { a: 0.32, ..light }, 0.),
            linear_color_stop(Hsla { a: 0., ..light }, 0.6),
        )),
        CARD_INNER_R,
    );

    let mut vivid: Vec<Hsla> = colors.iter().copied().filter(|c| c.l > 0.1 && c.l < 0.95).collect();
    vivid.sort_by(|a, b| b.s.total_cmp(&a.s));
    let boost = |c: Hsla| Hsla { s: c.s.max(0.55), l: 0.6, a: 0.2, ..c };
    let (a, b) = match vivid.as_slice() {
        [a, b, ..] => (boost(*a), boost(*b)),
        [a] => (boost(*a), Hsla { h: (a.h + 0.15).rem_euclid(1.), ..boost(*a) }),
        [] => {
            let hue = (rx * 0.6 + ry * 0.4).rem_euclid(1.);
            (hsla(hue, 0.9, 0.6, 0.12), hsla((hue + 0.35).rem_euclid(1.), 0.9, 0.6, 0.12))
        }
    };
    // The sweep slides with the pointer so the colors shimmer as it moves.
    let sheen = radius(
        div().absolute().top_0().left_0().size_full().bg(linear_gradient(
            angle + 90.,
            linear_color_stop(a, (rx * 0.5).clamp(0., 0.5)),
            linear_color_stop(b, (0.5 + ry * 0.5).clamp(0.5, 1.)),
        )),
        CARD_INNER_R,
    );
    [glare.into_any_element(), sheen.into_any_element()]
}

#[cfg(test)]
mod tests {
    use super::{hue_key, justify};
    use gpui_kit::hsla;

    #[test]
    fn full_rows_fill_width_exactly_enough() {
        let rows = justify(&[1.5, 1.5, 0.56, 3.5, 1.78, 1.0], 900., 200., 10.);
        let first = &rows[0];
        let used: f32 = first.iter().map(|(_, w)| w).sum::<f32>() + 10. * (first.len() - 1) as f32;
        assert!((used - 900.).abs() <= first.len() as f32, "row width {used}");
        let count: usize = rows.iter().map(Vec::len).sum();
        assert_eq!(count, 6);
    }

    #[test]
    fn hue_key_puts_grays_after_colors() {
        let red = hsla(0.0, 0.8, 0.5, 1.);
        let gray = hsla(0.0, 0.0, 0.5, 1.);
        assert!(hue_key(&[red]) < hue_key(&[gray]));
    }
}
