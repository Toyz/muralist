//! The two side headers, in the same field-shaped language as the middle
//! bar. The sidebar's carries the app itself: a logo that is a live
//! triptych of what's on your screens right now, a wordmark with a sheen
//! running through it, and a ticker of facts underneath. The inspector's
//! carries the selection: its id decoded into place, its palette, and the
//! window buttons tinted by it.

use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Item, Muralist, Pal, bar_field, boxed, file_name, hex, icon, no_drag, pal, purity_color, radius, segmented_track,
    thousands, with_alpha,
};

/// How long each ticker fact stays up.
const TICK_SECS: u64 = 4;
const SCRAMBLE: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";

/// `id` partway through decoding: characters settle left to right, the rest
/// still flicker through random glyphs.
fn scrambled(id: &str, t: f32) -> String {
    let n = id.chars().count().max(1) as f32;
    let frame = (t * 40.) as u64;
    id.chars()
        .enumerate()
        .map(|(i, ch)| {
            let settle = 0.15 + 0.85 * (i as f32 / n);
            if t >= settle || !ch.is_ascii_alphanumeric() {
                ch
            } else {
                let h = (i as u64 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ frame.wrapping_mul(0xbf58_476d_1ce4_e5b9);
                SCRAMBLE[(h >> 33) as usize % SCRAMBLE.len()] as char
            }
        })
        .collect()
}

impl Muralist {
    /// Logo, wordmark, ticker, settings.
    pub(super) fn render_side_head(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = pal();

        // What's on each screen right now, left to right.
        let mut screens = self.monitors.clone();
        screens.sort_by_key(|m| (m.x, m.y));
        let on: Vec<PathBuf> = screens.iter().filter_map(|m| self.applied.desktop.get(&m.name).cloned()).collect();
        let now: Vec<PathBuf> = on.iter().filter_map(|path| self.local_thumb(path)).collect();
        let tints: Vec<Hsla> =
            on.iter().filter_map(|path| self.local_palettes.get(path).and_then(|c| c.first().copied())).collect();

        // The logo: a rim of the turning gradient around a triptych of the screens.
        const LOGO: f32 = 28.;
        let inner = LOGO - 4.;
        let slices = now.len().max(1);
        let slice_w = (inner - (slices as f32 - 1.)) / slices as f32;
        let logo = div()
            .relative()
            .flex_none()
            .size(px(LOGO))
            .rounded_lg()
            .with_animation("logo-spin", Animation::new(Duration::from_secs(8)).repeat(), move |d, t| {
                d.bg(linear_gradient(
                    135. + t * 360.,
                    linear_color_stop(p.accent, 0.),
                    linear_color_stop(hex(0x22d3ee), 1.),
                ))
            })
            .when(!now.is_empty(), |d| {
                d.child(
                    h_flex()
                        .absolute()
                        .top(px(2.))
                        .left(px(2.))
                        .size(px(inner))
                        .gap(px(1.))
                        .rounded_md()
                        .bg(hsla(0., 0., 0., 0.6))
                        .children(now.iter().cloned().enumerate().map(move |(i, path)| {
                            // Each pane breathes in turn, like screens waking.
                            div()
                                .child(radius(boxed(img(path), slice_w, inner).object_fit(ObjectFit::Cover), 3.))
                                .with_animation(
                                    SharedString::from(format!("logo-pane-{i}")),
                                    Animation::new(Duration::from_millis(3600)).repeat(),
                                    move |d, t| {
                                        let phase = (t - i as f32 / slices as f32).rem_euclid(1.);
                                        let glow = (1. - (phase * 2. - 1.).abs()).powi(3);
                                        d.opacity(0.7 + 0.3 * glow)
                                    },
                                )
                        })),
                )
            });

        // The wordmark, with a band of light running through it.
        let word = "muralist";
        let len = word.len() as f32;
        let letters = word.chars().enumerate().map(move |(i, ch)| {
            div().child(ch.to_string()).with_animation(
                SharedString::from(format!("wordmark-{i}")),
                Animation::new(Duration::from_millis(4200)).repeat(),
                move |d, t| {
                    // The band sweeps past the end, then rests a while.
                    let at = t * (len + 8.) - 2.;
                    let near = (1. - ((i as f32 - at).abs() / 2.2)).max(0.);
                    let e = near * near * (3. - 2. * near);
                    let lit = Hsla { l: (p.accent.l + 0.25).min(0.92), ..p.accent };
                    d.text_color(p.fg.blend(Hsla { a: e, ..lit }))
                },
            )
        });

        // The ticker: one fact at a time, sliding up.
        let mut facts: Vec<(&'static str, String)> =
            vec![("monitor", format!("{} screen{} live", screens.len(), if screens.len() == 1 { "" } else { "s" }))];
        if let Some(countdown) = self.rotation_countdown() {
            facts.push(("shuffle", format!("rotation {countdown}")));
        }
        if !self.library.is_empty() {
            facts.push(("folder", format!("{} saved", thousands(self.library.len() as u64))));
        }
        if !self.applied.history.is_empty() {
            facts.push(("calendar", format!("{} sets in history", thousands(self.applied.history.len() as u64))));
        }
        if !self.galleries.list.is_empty() {
            let n = self.galleries.list.len();
            facts.push(("gallery-vertical-end", format!("{n} galler{}", if n == 1 { "y" } else { "ies" })));
        }
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let tick = (secs / TICK_SECS) as usize;
        let (fact_icon, fact) = facts[tick % facts.len()].clone();
        let ticker = h_flex().h(px(13.)).overflow_hidden().child(
            h_flex()
                .gap_1()
                .text_size(px(10.))
                .text_color(p.dim)
                .child(icon(fact_icon).size(px(10.)).text_color(with_alpha(p.accent, 0.9)))
                .child(fact)
                .with_animation(
                    SharedString::from(format!("ticker-{tick}")),
                    Animation::new(Duration::from_millis(420)).with_easing(ease_out_quint()),
                    |d, t| d.mt(px(12. * (1. - t))).opacity(t),
                ),
        );

        // Settings: a gear that idles slowly and spins up under the pointer.
        let spin = if self.gear_hover { 1400 } else { 14000 };
        let gear = segmented_track(p).flex_none().child(
            div()
                .id("open-settings")
                .size(px(24.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_md()
                .cursor_pointer()
                .text_color(if self.gear_hover { p.fg } else { p.muted })
                .hover(move |s| s.bg(with_alpha(p.accent, 0.2)))
                .tooltip(|window, cx| Tooltip::new("Settings").build(window, cx))
                .child(icon("settings").size_3p5().with_animation(
                    SharedString::from(format!("gear-{spin}")),
                    Animation::new(Duration::from_millis(spin)).repeat(),
                    |i, t| i.rotate(percentage(t)),
                ))
                .on_hover(cx.listener(|this, over: &bool, _, cx| {
                    this.gear_hover = *over;
                    cx.notify();
                }))
                .on_click(cx.listener(|this, _, window, cx| this.open_settings(window, cx))),
        );

        h_flex()
            .flex_1()
            .min_w_0()
            .gap_2()
            .child(
                bar_field("side-field", &tints, p).pl(px(5.)).gap_2p5().child(logo).child(
                    v_flex()
                        .min_w_0()
                        .gap_0()
                        .child(
                            h_flex()
                                .text_size(px(15.))
                                .line_height(px(17.))
                                .font_weight(FontWeight::BOLD)
                                .children(letters),
                        )
                        .child(ticker),
                ),
            )
            .child(no_drag(gear))
    }

    /// The selection's id and palette, then the window buttons.
    pub(super) fn render_inspector_head(&mut self, controls: Option<AnyElement>) -> impl IntoElement + use<> {
        let p = pal();
        let colors = self.selected_colors();
        let (key, thumb, line, purity) = match self.selected.clone() {
            Some(Item::Remote(w)) => {
                let thumb = self.remote_thumb(&w, false);
                let line = format!("{}  ·  {}", w.resolution, w.category);
                (Some(format!("wallhaven-{}", w.id)), thumb, line, Some(w.purity.clone()))
            }
            Some(Item::Local(path)) => {
                let thumb = self.local_thumb(&path);
                (Some(file_name(&path)), thumb, "saved on this machine".to_owned(), None)
            }
            None => (None, None, String::new(), None),
        };

        let field = bar_field("inspector-field", &colors, p).pl(px(5.)).gap_2p5();
        let field = match key {
            None => field
                .child(
                    div()
                        .flex_none()
                        .size(px(28.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_md()
                        .border_1()
                        .border_dashed()
                        .border_color(p.border_strong)
                        .child(
                            icon("crosshair").size_3p5().text_color(p.dim).with_animation(
                                "inspector-idle",
                                Animation::new(Duration::from_millis(1600))
                                    .repeat()
                                    .with_easing(pulsating_between(0.35, 1.)),
                                |d, t| d.opacity(t),
                            ),
                        ),
                )
                .child(
                    v_flex()
                        .min_w_0()
                        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("Details"))
                        .child(div().text_size(px(10.)).text_color(p.dim).child("nothing picked yet")),
                ),
            Some(key) => {
                let ring = colors.first().copied().unwrap_or(p.accent);
                let decoding = key.clone();
                field
                    .child(
                        div()
                            .flex_none()
                            .size(px(28.))
                            .rounded_md()
                            .bg(p.bg)
                            .when_some(thumb, |d, t| {
                                d.child(radius(boxed(img(t), 28., 28.).object_fit(ObjectFit::Cover), 6.))
                            })
                            .child(div().absolute().inset_0().rounded_md().border_1().border_color(ring))
                            .relative(),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                h_flex()
                                    .min_w_0()
                                    .gap_1p5()
                                    .children(purity.map(|purity| {
                                        div().flex_none().size(px(6.)).rounded_full().bg(purity_color(&purity))
                                    }))
                                    .child(
                                        div()
                                            .min_w_0()
                                            .truncate()
                                            .text_sm()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .with_animation(
                                                SharedString::from(format!("decode-{key}")),
                                                Animation::new(Duration::from_millis(560)),
                                                move |d, t| d.child(scrambled(&decoding, t)),
                                            ),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .min_w_0()
                                    .gap_2()
                                    // The palette, as one segmented strip.
                                    .child(h_flex().flex_none().gap(px(1.)).children(
                                        colors.iter().take(6).enumerate().map(|(i, c)| {
                                            div().w(px(9.)).h(px(4.)).rounded_sm().bg(*c).with_animation(
                                                SharedString::from(format!("swatch-{key}-{i}")),
                                                Animation::new(Duration::from_millis(220 + i as u64 * 70))
                                                    .with_easing(ease_out_quint()),
                                                move |d, t| {
                                                    let t = ((t * (220. + i as f32 * 70.) - i as f32 * 70.) / 220.)
                                                        .clamp(0., 1.);
                                                    d.w(px(9. * t)).opacity(t)
                                                },
                                            )
                                        }),
                                    ))
                                    .child(div().min_w_0().truncate().text_size(px(10.)).text_color(p.dim).child(line)),
                            ),
                    )
            }
        };

        h_flex().flex_1().min_w_0().gap_2().child(no_drag(field)).children(controls)
    }
}

/// The window buttons in a track, tinted by the palette.
pub(super) fn window_controls(window: &Window, p: Pal) -> AnyElement {
    let ctrl = |id: &'static str, name: &str, danger: bool, area: WindowControlArea| {
        no_drag(
            div()
                .id(id)
                .window_control_area(area)
                .size(px(24.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_md()
                .text_color(p.muted)
                .cursor_pointer()
                .hover(move |s| {
                    if danger {
                        s.bg(p.error).text_color(white())
                    } else {
                        s.bg(with_alpha(p.accent, 0.25)).text_color(p.fg)
                    }
                })
                .child(icon(name).size_3p5()),
        )
    };
    let max_icon = if window.is_maximized() { "window-restore" } else { "window-maximize" };
    segmented_track(p)
        .flex_none()
        .child(
            ctrl("win-min", "window-minimize", false, WindowControlArea::Min)
                .on_click(|_, window, _| window.minimize_window()),
        )
        .child(ctrl("win-max", max_icon, false, WindowControlArea::Max).on_click(|_, window, _| window.zoom_window()))
        .child(div().w(px(1.)).h(px(14.)).mx_0p5().bg(p.border_strong))
        .child(ctrl("win-close", "window-close", true, WindowControlArea::Close).on_click(|_, _, cx| cx.quit()))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::scrambled;

    #[test]
    fn scramble_settles() {
        assert_eq!(scrambled("wallhaven-e8vge8", 1.), "wallhaven-e8vge8");
        assert_eq!(scrambled("wallhaven-e8vge8", 0.).len(), "wallhaven-e8vge8".len());
        // Punctuation never scrambles.
        assert_eq!(scrambled("a-b", 0.).chars().nth(1), Some('-'));
    }
}
