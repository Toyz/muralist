//! Side panels as frosted glass over the selected wallpaper, ambilight
//! glows, and staggered entrances.

use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::*;

use super::{Item, Muralist, Pal, fade_in, local_key, with_alpha};
use crate::thumbs::Job;

/// Most vivid color of a palette, lifted into a glow.
pub(super) fn glow_color(colors: &[Hsla]) -> Option<Hsla> {
    let vivid = colors
        .iter()
        .copied()
        .filter(|c| c.l > 0.08 && c.l < 0.95)
        .max_by(|a, b| (a.s * (1. - (a.l - 0.5).abs())).total_cmp(&(b.s * (1. - (b.l - 0.5).abs()))))?;
    Some(Hsla { h: vivid.h, s: vivid.s.max(0.45), l: 0.55, a: 1. })
}

/// Big soft colored shadow, like a TV's backlight.
pub(super) fn ambilight(c: Hsla, blur: f32, alpha: f32) -> Vec<BoxShadow> {
    vec![BoxShadow {
        color: with_alpha(c, alpha),
        offset: point(px(0.), px(blur * 0.2)),
        blur_radius: px(blur),
        spread_radius: px(-blur * 0.1),
        inset: false,
    }]
}

/// Layers to place first inside a panel: blurred wallpaper, then glass.
pub(super) fn glass_layers(backdrop: Option<(PathBuf, String)>, panel: &str, p: Pal) -> Vec<AnyElement> {
    let mut layers = Vec::new();
    if let Some((path, key)) = backdrop {
        layers.push(
            fade_in(
                SharedString::from(format!("glass-{panel}-{key}")),
                img(path).size_full().object_fit(ObjectFit::Cover),
            )
            .into_any_element(),
        );
    }
    layers.push(div().absolute().top_0().left_0().size_full().bg(with_alpha(p.surface, 0.8)).into_any_element());
    // A faint sheen along the top, like light on glass.
    layers.push(
        div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(220.))
            .bg(linear_gradient(
                180.,
                linear_color_stop(hsla(0., 0., 1., 0.045), 0.),
                linear_color_stop(hsla(0., 0., 1., 0.), 1.),
            ))
            .into_any_element(),
    );
    layers
}

/// Cubic ease-out, the curve most entrances here use.
pub(super) fn ease_out_cubic(t: f32) -> f32 {
    1. - (1. - t.clamp(0., 1.)).powi(3)
}

/// An animation that waits `delay_ms`, then runs 0 to 1 over `dur_ms`:
/// the progress its closure sees is already offset by the delay.
pub(super) fn delayed(delay_ms: u64, dur_ms: u64) -> Animation {
    let (delay, dur) = (delay_ms as f32, dur_ms.max(1) as f32);
    Animation::new(Duration::from_millis(delay_ms + dur_ms))
        .with_easing(move |t| ((t * (delay + dur) - delay) / dur).clamp(0., 1.))
}

/// Fade-and-rise entrance, delayed by `index` steps.
pub(super) fn stagger(id: SharedString, index: usize, child: impl IntoElement) -> impl IntoElement {
    let delay = index as f32 * 0.08;
    let dur = 0.36;
    div().child(child).with_animation(
        id,
        Animation::new(Duration::from_secs_f32(delay + dur))
            .with_easing(move |t| ((t * (delay + dur) - delay) / dur).clamp(0., 1.)),
        |d, t| {
            let e = 1. - (1. - t).powi(3);
            d.opacity(e).mt(px(14. * (1. - e)))
        },
    )
}

/// A new selection settling into a panel: a quick lift into place from
/// partly faded, offset without moving anything around it.
pub(super) fn settle(id: SharedString, child: impl IntoElement) -> impl IntoElement {
    div().relative().child(child).with_animation(id, Animation::new(Duration::from_millis(300)), |d, t| {
        let e = 1. - (1. - t).powi(3);
        d.opacity(0.3 + 0.7 * e).top(px(10. * (1. - e)))
    })
}

impl Muralist {
    /// The blurred backdrop of the selection, once baked.
    pub(super) fn backdrop(&mut self) -> Option<(PathBuf, String)> {
        let item = self.selected.clone()?;
        let (src, key) = match &item {
            Item::Remote(w) => (self.remote_thumb(w, true)?, w.id.clone()),
            Item::Local(path) => (self.local_thumb(path)?, local_key(path)),
        };
        let path = self.thumb(format!("blur-{key}"), |k| Job::Blur { key: k, path: src })?;
        Some((path, key))
    }

    /// Glow color for an image on disk, from its thumbnail's palette.
    pub(super) fn path_glow(&mut self, path: &Path) -> Option<Hsla> {
        glow_color(self.local_palette(path)?)
    }
}
