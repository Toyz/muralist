//! The fit lab: how well a wallpaper suits these screens, worked out two
//! ways. Span lays the desk over the picture, the screens dropping into
//! place on it, the part that won't be seen tinted red and tallied. Fill
//! shows each screen's own crop of it. Either way every screen gets a gauge
//! whose needle swings to how many source pixels it gets per pixel it shows,
//! and the lot is boiled down to one score that counts itself up.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::glass::{delayed, ease_out_cubic};
use super::{Muralist, boxed, develop, glass, icon, pal, purity_color, section, with_alpha};
use crate::wallhaven::Wallpaper;

/// Which way the lab looks at the fit.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum FitMode {
    #[default]
    Span,
    Fill,
}

/// Gauge range, as a scale factor: a quarter (heavily supersampled) to 3x
/// (badly upscaled), on a log scale so 1x sits in the middle.
const GAUGE_MIN: f32 = 0.25;
const GAUGE_MAX: f32 = 3.;
const DIAGRAM_MAX_H: f32 = 170.;

/// How good a scale factor looks: 1 when every screen pixel has a source
/// pixel (or more), falling away as the image is stretched.
fn quality(scale: f32) -> f32 {
    if scale <= 1. { 1. } else { (1. / scale).powf(1.3) }
}

fn verdict(score: u32) -> &'static str {
    match score {
        90.. => "PERFECT FIT",
        70.. => "SHARP",
        45.. => "SOFT",
        _ => "BLURRY",
    }
}

fn scale_word(scale: f32) -> &'static str {
    if scale <= 0.75 {
        "supersampled"
    } else if scale <= 1. {
        "native"
    } else if scale <= 1.5 {
        "soft"
    } else {
        "blurry"
    }
}

/// Position of `scale` along a gauge, 0 to 1.
fn gauge_at(scale: f32) -> f32 {
    let (lo, hi) = (GAUGE_MIN.ln(), GAUGE_MAX.ln());
    ((scale.clamp(GAUGE_MIN, GAUGE_MAX).ln() - lo) / (hi - lo)).clamp(0., 1.)
}

impl Muralist {
    fn sharpness_color(&self, scale: f32) -> Hsla {
        let p = pal();
        if scale <= 1. {
            p.ok
        } else if scale <= 1.5 {
            purity_color("sketchy")
        } else {
            p.error
        }
    }

    pub(super) fn render_fit_check(&mut self, w: &Wallpaper, c: Hsla, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = pal();
        let (iw, ih) = w.dims().map(|(a, b)| (a as f32, b as f32))?;
        if self.monitors.is_empty() {
            return None;
        }
        let thumb = self.remote_thumb(w, true).or_else(|| self.remote_thumb(w, false));
        let monitors = self.monitors.clone();
        let mode = if monitors.len() < 2 { FitMode::Fill } else { self.fit_mode };
        let key = &w.id;

        // ---- the numbers ----
        // Fill: each screen covered on its own.
        let fill: Vec<f32> = monitors.iter().map(|m| (m.px_width as f32 / iw).max(m.px_height as f32 / ih)).collect();
        // Span: one image across the whole desk.
        let (min_x, min_y, max_x, max_y) = crate::monitors::extent(&monitors);
        let (bw, bh) = ((max_x - min_x) as f32, (max_y - min_y) as f32);
        let density = |m: &crate::monitors::Monitor| m.px_width as f32 / m.width.max(1) as f32;
        let px_per_unit = monitors.iter().map(density).fold(1., f32::max);
        let span_scale = (bw * px_per_unit / iw).max(bh * px_per_unit / ih);
        let span: Vec<f32> = monitors.iter().map(|m| span_scale * density(m) / px_per_unit).collect();
        let cropped = (1. - (bw * bh) / ((iw * span_scale / px_per_unit) * (ih * span_scale / px_per_unit))).max(0.);

        let fill_score = (fill.iter().map(|s| quality(*s)).fold(1., f32::min) * 100.).round() as u32;
        let span_score =
            (span.iter().map(|s| quality(*s)).fold(1., f32::min) * (1. - cropped * 0.6) * 100.).round() as u32;
        let scales = match mode {
            FitMode::Fill => &fill,
            FitMode::Span => &span,
        };

        // ---- the switch, with each mode's score ----
        let tab = |id: &'static str, label: &'static str, n: u32, m: FitMode| {
            let on = mode == m;
            let tint = if n >= 70 {
                p.ok
            } else if n >= 45 {
                purity_color("sketchy")
            } else {
                p.error
            };
            v_flex()
                .id(id)
                .flex_1()
                .px_2p5()
                .py_1p5()
                .rounded_lg()
                .cursor_pointer()
                .border_1()
                .border_color(if on { with_alpha(c, 0.7) } else { p.border })
                .bg(if on { with_alpha(c, 0.12) } else { p.bg })
                .child(
                    h_flex()
                        .justify_between()
                        .text_size(px(9.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(if on { c } else { p.dim })
                        .child(label)
                        .child(div().text_color(tint).child(verdict(n))),
                )
                .child(
                    h_flex()
                        .items_end()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(22.))
                                .line_height(px(24.))
                                .font_weight(FontWeight::BOLD)
                                .text_color(if on { p.fg } else { p.muted })
                                .with_animation(
                                    SharedString::from(format!("fit-score-{key}-{label}")),
                                    Animation::new(std::time::Duration::from_millis(900)).with_easing(ease_out_quint()),
                                    move |d, t| d.child(format!("{}", (n as f32 * t).round() as u32)),
                                ),
                        )
                        .child(div().pb_0p5().text_xs().text_color(p.dim).child("/ 100")),
                )
                // A thin bar under the number, filled to the score.
                .child(div().mt_1().h(px(3.)).w_full().rounded_full().bg(p.surface2).child(
                    div().h_full().rounded_full().bg(tint).with_animation(
                        SharedString::from(format!("fit-bar-{key}-{label}")),
                        Animation::new(std::time::Duration::from_millis(900)).with_easing(ease_out_quint()),
                        move |d, t| d.w(relative(n as f32 / 100. * t)),
                    ),
                ))
                .when(!on, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| {
                        this.fit_mode = m;
                        cx.notify();
                    }))
                })
        };
        let switch = h_flex()
            .gap_2()
            .when(monitors.len() > 1, |d| d.child(tab("fit-span", "SPAN", span_score, FitMode::Span)))
            .child(tab("fit-fill", "FILL", fill_score, FitMode::Fill));

        // ---- the diagram ----
        let full_w = self.details_w;
        let diagram: AnyElement = match mode {
            FitMode::Span => {
                // The whole picture, with the desk laid over it where a span
                // would put it.
                let dk = (full_w / iw).min(DIAGRAM_MAX_H / ih);
                let (dw, dh) = (iw * dk, ih * dk);
                let s_logical = (bw / iw).max(bh / ih);
                let (win_w, win_h) = (bw / s_logical * dk, bh / s_logical * dk);
                let (win_x, win_y) = ((dw - win_w) / 2., (dh - win_h) / 2.);
                let screens: Vec<AnyElement> = monitors
                    .iter()
                    .enumerate()
                    .map(|(i, m)| {
                        let x = win_x + (m.x - min_x) as f32 / s_logical * dk;
                        let y = win_y + (m.y - min_y) as f32 / s_logical * dk;
                        let (sw, sh) = (m.width as f32 / s_logical * dk, m.height as f32 / s_logical * dk);
                        let tint = self.sharpness_color(span[i]);
                        div()
                            .absolute()
                            .left(px(x))
                            .w(px(sw))
                            .h(px(sh))
                            .overflow_hidden()
                            .when_some(thumb.clone(), |d, t| {
                                d.child(boxed(img(t), dw, dh).absolute().left(px(-x)).top(px(-y)))
                            })
                            .child(div().absolute().inset_0().border_1().border_color(tint))
                            .child(
                                div()
                                    .absolute()
                                    .top_1()
                                    .left_1()
                                    .px_1()
                                    .rounded_sm()
                                    .bg(hsla(0., 0., 0., 0.7))
                                    .text_size(px(9.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(tint)
                                    .child(format!("{} {:.1}x", m.name, span[i])),
                            )
                            // Each screen drops into place on the picture.
                            .with_animation(
                                SharedString::from(format!("fit-drop-{key}-{i}")),
                                delayed(150 + i as u64 * 110, 380),
                                move |d, t| {
                                    let e = ease_out_cubic(t);
                                    d.top(px(y - 14. * (1. - e))).opacity(e)
                                },
                            )
                            .into_any_element()
                    })
                    .collect();
                div()
                    .relative()
                    .w(px(dw))
                    .h(px(dh))
                    .rounded_md()
                    .overflow_hidden()
                    .bg(p.bg)
                    // What a span throws away, dimmed and tinted red.
                    .when_some(thumb.clone(), |d, t| d.child(boxed(img(t), dw, dh).grayscale(true).opacity(0.3)))
                    .child(div().absolute().inset_0().bg(with_alpha(p.error, 0.14)))
                    .children(develop::thirds(hsla(0., 0., 1., 0.08)))
                    .children(screens)
                    // The desk's outline, bracketed.
                    .child(
                        div()
                            .absolute()
                            .left(px(win_x - 6.))
                            .top(px(win_y - 6.))
                            .w(px(win_w + 12.))
                            .h(px(win_h + 12.))
                            .children((0..4).map(|i| develop::bracket(i, 0., with_alpha(c, 0.9)))),
                    )
                    .child(develop::beam(
                        SharedString::from(format!("fit-beam-{key}")),
                        develop::Beam { color: c, band: 30., glow: 0.16, line: 0.6, ms: 2600 },
                        dh,
                    ))
                    .when(cropped > 0.005, |d| {
                        d.child(
                            div()
                                .absolute()
                                .bottom_1()
                                .right_1()
                                .px_1p5()
                                .rounded_sm()
                                .bg(hsla(0., 0., 0., 0.75))
                                .border_1()
                                .border_color(with_alpha(p.error, 0.6))
                                .text_size(px(9.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(p.error)
                                .child(format!("-{:.0}% CROPPED", cropped * 100.)),
                        )
                    })
                    .into_any_element()
            }
            FitMode::Fill => {
                // Each screen's own crop: the whole picture dimmed, the part
                // that screen keeps lit, side by side at their real shapes.
                let gap = 8.;
                let n = monitors.len() as f32;
                let tile_h = 76_f32;
                let total: f32 = monitors.iter().map(|m| tile_h * m.width as f32 / m.height.max(1) as f32).sum();
                let fit = ((full_w - gap * (n - 1.)) / total).min(1.);
                let tiles: Vec<AnyElement> = monitors
                    .iter()
                    .enumerate()
                    .map(|(i, m)| {
                        let th = tile_h * fit;
                        let tw = th * m.width as f32 / m.height.max(1) as f32;
                        let tint = self.sharpness_color(fill[i]);
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .relative()
                                    .w(px(tw))
                                    .h(px(th))
                                    .rounded_md()
                                    .overflow_hidden()
                                    .bg(p.bg)
                                    .when_some(thumb.clone(), |d, t| {
                                        d.child(boxed(img(t), tw, th).object_fit(ObjectFit::Cover))
                                    })
                                    .child(div().absolute().inset_0().rounded_md().border_1().border_color(tint)),
                            )
                            .child(
                                h_flex()
                                    .justify_between()
                                    .text_size(px(9.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(div().text_color(p.dim).child(m.name.clone()))
                                    .child(div().text_color(tint).child(format!("{:.1}x", fill[i]))),
                            )
                            .with_animation(
                                SharedString::from(format!("fit-tile-{key}-{i}")),
                                delayed(120 + i as u64 * 100, 360),
                                |d, t| {
                                    let e = ease_out_cubic(t);
                                    d.opacity(e).mt(px(10. * (1. - e)))
                                },
                            )
                            .into_any_element()
                    })
                    .collect();
                h_flex().gap(px(gap)).items_start().justify_center().children(tiles).into_any_element()
            }
        };

        // ---- a gauge per screen ----
        let gauges: Vec<AnyElement> = monitors
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let s = scales[i];
                let tint = self.sharpness_color(s);
                let at = gauge_at(s);
                let one = gauge_at(1.);
                let track = div()
                    .relative()
                    .w_full()
                    .h(px(8.))
                    .rounded_full()
                    .bg(linear_gradient(
                        90.,
                        linear_color_stop(with_alpha(p.ok, 0.55), one),
                        linear_color_stop(with_alpha(p.error, 0.55), 1.),
                    ))
                    // 1x marked: left of it every screen pixel has a source pixel.
                    .child(div().absolute().top(px(-2.)).bottom(px(-2.)).left(relative(one)).w(px(1.)).bg(p.fg))
                    .child(
                        div()
                            .absolute()
                            .top(px(-3.))
                            .w(px(3.))
                            .h(px(14.))
                            .rounded_full()
                            .bg(white())
                            .shadow(glass::ambilight(tint, 8., 0.9))
                            .with_animation(
                                SharedString::from(format!("fit-needle-{key}-{i}-{}", mode == FitMode::Span)),
                                delayed(200 + i as u64 * 90, 700),
                                move |d, t| {
                                    // Swings past and settles, like a meter.
                                    let e = ease_out_cubic(t);
                                    let wobble = (t * 9.).sin() * (1. - t) * 0.04;
                                    d.left(relative((at * e + wobble).clamp(0., 1.)))
                                },
                            ),
                    );
                v_flex()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_1p5()
                            .text_size(px(10.))
                            .child(icon("monitor").size_3().text_color(tint))
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(m.name.clone()))
                            .child(div().text_color(p.dim).child(format!("{}x{}", m.px_width, m.px_height)))
                            .child(div().flex_1())
                            .child(div().text_color(tint).font_weight(FontWeight::SEMIBOLD).child(scale_word(s)))
                            .child(div().text_color(p.dim).child(format!("{:.2} src px / px", 1. / s.max(0.01)))),
                    )
                    .child(track)
                    .into_any_element()
            })
            .collect();
        let scale_legend = h_flex()
            .justify_between()
            .text_size(px(9.))
            .text_color(p.dim)
            .child("supersampled")
            .child("native")
            .child("blurry");

        Some(
            section("Fit lab", p)
                .child(switch)
                .child(h_flex().justify_center().child(diagram))
                .child(v_flex().gap_2p5().children(gauges).child(scale_legend))
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{gauge_at, quality, verdict};

    #[test]
    fn native_is_perfect_and_stretching_costs() {
        assert_eq!(quality(0.5), 1.);
        assert_eq!(quality(1.), 1.);
        assert!(quality(2.) < quality(1.5));
        assert_eq!(verdict(100), "PERFECT FIT");
        assert_eq!(verdict(10), "BLURRY");
    }

    #[test]
    fn the_gauge_is_logarithmic_around_one() {
        assert!((gauge_at(1.) - (0.25f32.ln().abs() / (3f32.ln() - 0.25f32.ln()))).abs() < 1e-4);
        assert_eq!(gauge_at(0.01), 0.);
        assert_eq!(gauge_at(99.), 1.);
    }
}
