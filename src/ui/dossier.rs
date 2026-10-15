//! The Info and Similar doors under the preview, as two little dossiers
//! instead of two buttons. Info shows its tags as a barcode, one bar per
//! tag in its purity's color, that runs as an equalizer while the file is
//! still being pulled. Similar shows a stack of the lookalikes themselves,
//! which fans out under the pointer. Both decode their headline into place.

use std::time::Duration;

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::details::Similar;
use super::{
    InfoState, InspectorTab, Item, Muralist, boxed, compact_count, glass, icon, pal, purity_color, radius, with_alpha,
};

const TILE_H: f32 = 64.;
const BARS: usize = 12;
const THUMB: (f32, f32) = (34., 24.);

/// A steady pseudo-random height for a tag's bar.
fn bar_h(name: &str) -> f32 {
    let h = name.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3));
    10. + (h % 25) as f32
}

/// Text that keeps scrambling, for things still on their way.
fn scrambling(id: SharedString, text: &'static str) -> impl IntoElement {
    div().with_animation(id, Animation::new(Duration::from_millis(900)).repeat(), move |d, t| {
        // Settles left to right, then starts over.
        d.child(super::headers::scrambled(text, t * 1.2))
    })
}

impl Muralist {
    pub(super) fn render_dossier(&mut self, item: &Item, cx: &mut Context<Self>) -> AnyElement {
        let p = pal();
        let id = match item {
            Item::Remote(w) => Some(w.id.clone()),
            Item::Local(path) => self.provenance.of(path).map(|r| r.wallpaper.id.clone()),
        };
        let key = id.clone().unwrap_or_else(|| "local".into());
        let c = glass::glow_color(&self.selected_colors()).unwrap_or(p.accent);
        let info = id.as_ref().and_then(|id| self.info.get(id));
        let similar = id.as_ref().and_then(|id| self.similar.get(id));

        let tile = |name: &'static str, hover: bool| {
            div()
                .id(name)
                .relative()
                .flex_1()
                .min_w_0()
                .h(px(TILE_H))
                .rounded_lg()
                .overflow_hidden()
                .cursor_pointer()
                .bg(linear_gradient(
                    120.,
                    linear_color_stop(with_alpha(c, if hover { 0.22 } else { 0.1 }), 0.),
                    linear_color_stop(p.bg, 0.7),
                ))
                .border_1()
                .border_color(if hover { c } else { p.border_strong })
        };
        let label = |icon_name: &'static str, text: &'static str| {
            h_flex()
                .gap_1()
                .text_size(px(9.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(p.dim)
                .child(icon(icon_name).size(px(10.)).text_color(c))
                .child(text)
        };
        // A beam crossing the tile while the pointer is on it.
        let beam = |name: &'static str| {
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .w(px(36.))
                .bg(linear_gradient(
                    90.,
                    linear_color_stop(with_alpha(c, 0.), 0.),
                    linear_color_stop(with_alpha(c, 0.25), 1.),
                ))
                .child(div().absolute().top_0().bottom_0().right_0().w(px(1.)).bg(with_alpha(c, 0.9)))
                .with_animation(
                    SharedString::from(format!("{name}-beam")),
                    Animation::new(Duration::from_millis(1400)).repeat().with_easing(ease_in_out),
                    |d, t| d.left(relative(-0.2 + 1.3 * t)),
                )
        };

        // ---- Info ----
        let info_hover = self.dossier_hover == Some(0);
        let (headline, sub, bars): (AnyElement, String, Vec<AnyElement>) = match info {
            // A file of unknown origin: nothing to pull.
            None if id.is_none() => {
                (div().child("this file").into_any_element(), "saved on this machine".into(), Vec::new())
            }
            Some(InfoState::Ready(i)) => {
                let n = i.tags.len();
                let by = i.uploader.as_ref().map(|u| format!(" · {}", u.username)).unwrap_or_default();
                let bars = i
                    .tags
                    .iter()
                    .take(BARS)
                    .enumerate()
                    .map(|(ix, t)| {
                        let h = bar_h(&t.name);
                        let col = purity_color(&t.purity);
                        div()
                            .w(px(3.))
                            .rounded_sm()
                            .bg(col)
                            .with_animation(
                                SharedString::from(format!("bar-{key}-{ix}")),
                                Animation::new(Duration::from_millis(300 + ix as u64 * 40)),
                                move |d, t| {
                                    let delay = ix as f32 * 40.;
                                    let t = ((t * (300. + delay) - delay) / 300.).clamp(0., 1.);
                                    let e = super::glass::ease_out_cubic(t);
                                    d.h(px(h * e))
                                },
                            )
                            .into_any_element()
                    })
                    .collect();
                (
                    div().child(format!("{n} tag{}", if n == 1 { "" } else { "s" })).into_any_element(),
                    format!("{} views{by}", compact_count(i.views)),
                    bars,
                )
            }
            Some(InfoState::Failed(_)) => {
                (div().child("offline").into_any_element(), "couldn't reach Wallhaven".into(), Vec::new())
            }
            // Still on its way: the barcode runs as an equalizer.
            _ => (
                scrambling(SharedString::from(format!("info-scr-{key}")), "decoding").into_any_element(),
                "pulling the file".into(),
                (0..BARS)
                    .map(|ix| {
                        div()
                            .w(px(3.))
                            .rounded_sm()
                            .bg(with_alpha(c, 0.55))
                            .with_animation(
                                SharedString::from(format!("eq-{key}-{ix}")),
                                Animation::new(Duration::from_millis(700 + (ix as u64 * 53) % 300)).repeat(),
                                move |d, t| {
                                    let phase = (t + ix as f32 * 0.17).rem_euclid(1.);
                                    let wave = (phase * std::f32::consts::TAU).sin() * 0.5 + 0.5;
                                    d.h(px(6. + 24. * wave))
                                },
                            )
                            .into_any_element()
                    })
                    .collect(),
            ),
        };
        let info_tile = tile("dossier-info", info_hover)
            .child(
                h_flex()
                    .size_full()
                    .px_2p5()
                    .gap_2()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(label("info", "INFO"))
                            .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).truncate().child(headline))
                            .child(div().text_size(px(10.)).text_color(p.dim).truncate().child(sub)),
                    )
                    .child(h_flex().flex_none().h(px(36.)).items_end().gap(px(2.)).children(bars)),
            )
            .when(info_hover, |d| d.child(beam("info")))
            .on_hover(Self::hover_slot(cx, |this| &mut this.dossier_hover, 0))
            .on_click(cx.listener(|this, _, window, cx| this.open_sheet(InspectorTab::Info, window, cx)));

        // ---- Similar ----
        let sim_hover = self.dossier_hover == Some(1);
        let failed = matches!(similar, Some(Similar::Failed));
        let lookalikes: Vec<_> = match similar {
            Some(Similar::Ready(list)) => list.iter().take(3).cloned().collect(),
            _ => Vec::new(),
        };
        let (headline, sub): (AnyElement, String) = match similar {
            Some(Similar::Ready(list)) if list.is_empty() => {
                (div().child("one of a kind").into_any_element(), "nothing else like it".into())
            }
            Some(Similar::Ready(list)) => (
                div()
                    .child(format!("{} lookalike{}", list.len(), if list.len() == 1 { "" } else { "s" }))
                    .into_any_element(),
                "by color and tags".into(),
            ),
            Some(Similar::Failed) => (div().child("offline").into_any_element(), "couldn't search".into()),
            _ => (
                scrambling(SharedString::from(format!("sim-scr-{key}")), "searching").into_any_element(),
                "comparing colors".into(),
            ),
        };
        let thumbs: Vec<Option<std::path::PathBuf>> = if lookalikes.is_empty() {
            vec![None; 3]
        } else {
            lookalikes.iter().map(|w| self.remote_thumb(w, false)).collect()
        };
        let (tw, th) = THUMB;
        let spread = if sim_hover { 24. } else { 12. };
        let stack_w = tw + spread * (thumbs.len().max(1) - 1) as f32;
        let loading = lookalikes.is_empty();
        let stack = div().relative().flex_none().w(px(stack_w)).h(px(th + 8.)).children(
            thumbs.into_iter().enumerate().rev().map(|(i, t)| {
                let card = div()
                    .absolute()
                    .top(px(if sim_hover { (i as f32 - 1.).abs() * 3. } else { 4. }))
                    .left(px(i as f32 * spread))
                    .w(px(tw))
                    .h(px(th))
                    .rounded_md()
                    .bg(hsla(0., 0., 1., 0.08))
                    .border_1()
                    .border_color(if sim_hover { with_alpha(c, 0.9) } else { hsla(0., 0., 1., 0.25) })
                    .shadow_md()
                    .when_some(t, |d, t| {
                        d.child(radius(boxed(img(t), tw - 2., th - 2.).object_fit(ObjectFit::Cover), 5.))
                    });
                if loading {
                    // Placeholders shuffling while it searches.
                    card.with_animation(
                        SharedString::from(format!("sim-shuffle-{key}-{i}")),
                        Animation::new(Duration::from_millis(1200)).repeat(),
                        move |d, t| {
                            let phase = (t - i as f32 * 0.22).rem_euclid(1.);
                            let lift = (phase * std::f32::consts::TAU).sin().max(0.);
                            d.top(px(4. - 4. * lift)).opacity(0.4 + 0.6 * lift)
                        },
                    )
                    .into_any_element()
                } else {
                    card.with_animation(
                        SharedString::from(format!("sim-fan-{key}-{i}-{sim_hover}")),
                        Animation::new(Duration::from_millis(260)).with_easing(ease_out_quint()),
                        move |d, t| d.opacity(0.5 + 0.5 * t),
                    )
                    .into_any_element()
                }
            }),
        );
        let has_similar = id.is_some();
        let sim_tile = has_similar.then(|| {
            tile("dossier-similar", sim_hover)
                .child(
                    h_flex()
                        .size_full()
                        .px_2p5()
                        .gap_2()
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap_0p5()
                                .child(label("images", "SIMILAR"))
                                .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).truncate().child(headline))
                                .child(div().text_size(px(10.)).text_color(p.dim).truncate().child(sub)),
                        )
                        .when(!failed, |d| d.child(stack)),
                )
                .when(sim_hover, |d| d.child(beam("similar")))
                .on_hover(Self::hover_slot(cx, |this| &mut this.dossier_hover, 1))
                .on_click(cx.listener(|this, _, window, cx| this.open_sheet(InspectorTab::Similar, window, cx)))
        });

        let bounds = self.dossier_bounds.clone();
        h_flex()
            .gap_2()
            .on_children_prepainted(move |b, _, _| *bounds.borrow_mut() = b)
            .child(info_tile)
            .children(sim_tile)
            .into_any_element()
    }
}
