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
use super::{Muralist, boxed, glass, icon, pal, purity_color, section, with_alpha};
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
                // Desktop units per image pixel when one image covers the desk,
                // and where the image sits relative to it (centred, overhanging).
                let s_logical = (bw / iw).max(bh / ih);
                let (img_w, img_h) = (iw * s_logical, ih * s_logical);
                let (ox, oy) = ((bw - img_w) / 2., (bh - img_h) / 2.);

                // ---- the source map: what of the picture survives ----
                // Full width: only a tall portrait picture is held back, so the
                // map never floats in empty space.
                let mk = (full_w / iw).min(260. / ih);
                let (mw, mh) = (iw * mk, ih * mk);
                let (kw, kh) = (bw / s_logical * mk, bh / s_logical * mk);
                let (kx, ky) = ((mw - kw) / 2., (mh - kh) / 2.);
                // Image pixels lost on each side of the kept strip.
                let (lost_x, lost_y) = (((iw - bw / s_logical) / 2.).max(0.), ((ih - bh / s_logical) / 2.).max(0.));
                let stripes = |w: f32, h: f32| {
                    // Red warning stripes over what a span throws away.
                    let n = ((w + h) / 7.).ceil() as usize;
                    div().absolute().inset_0().overflow_hidden().bg(with_alpha(p.error, 0.12)).children((0..n).map(
                        move |i| {
                            div()
                                .absolute()
                                .top_0()
                                .bottom_0()
                                .left(px(i as f32 * 7. - h))
                                .w(px(2.))
                                .bg(with_alpha(p.error, 0.35))
                        },
                    ))
                };
                let lost_band = |x: f32, y: f32, w: f32, h: f32, label: String| {
                    div().absolute().left(px(x)).top(px(y)).w(px(w)).h(px(h)).child(stripes(w, h)).when(
                        w > 40. && h > 12.,
                        |d| {
                            d.child(
                                div()
                                    .absolute()
                                    .inset_0()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(9.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(p.error)
                                    .child(label),
                            )
                        },
                    )
                };
                let map = div()
                    .relative()
                    .w(px(mw))
                    .h(px(mh))
                    .rounded_md()
                    .overflow_hidden()
                    .bg(p.bg)
                    .when_some(thumb.clone(), |d, t| d.child(boxed(img(t), mw, mh).grayscale(true).opacity(0.35)))
                    // The strip a span keeps, lit.
                    .child(
                        div()
                            .absolute()
                            .left(px(kx))
                            .top(px(ky))
                            .w(px(kw))
                            .h(px(kh))
                            .overflow_hidden()
                            .when_some(thumb.clone(), |d, t| {
                                d.child(boxed(img(t), mw, mh).absolute().left(px(-kx)).top(px(-ky)))
                            })
                            .child(div().absolute().inset_0().border_1().border_color(with_alpha(c, 0.9))),
                    )
                    .when(lost_y >= 1., |d| {
                        let label = format!("{:.0} px cut", lost_y);
                        d.child(lost_band(0., 0., mw, ky, label.clone())).child(lost_band(
                            0.,
                            ky + kh,
                            mw,
                            mh - ky - kh,
                            label,
                        ))
                    })
                    .when(lost_x >= 1., |d| {
                        let label = format!("{:.0} px", lost_x);
                        d.child(lost_band(0., 0., kx, mh, label.clone())).child(lost_band(
                            kx + kw,
                            0.,
                            mw - kx - kw,
                            mh,
                            label,
                        ))
                    })
                    .when(cropped > 0.005, |d| {
                        d.child(
                            div()
                                .absolute()
                                .bottom_1()
                                .right_1()
                                .px_1p5()
                                .rounded_sm()
                                .bg(hsla(0., 0., 0., 0.8))
                                .border_1()
                                .border_color(with_alpha(p.error, 0.6))
                                .text_size(px(9.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(p.error)
                                .child(format!("-{:.0}% OF THE PICTURE", cropped * 100.)),
                        )
                    });

                // ---- the desk: each screen showing its own slice ----
                const STAND: f32 = 12.;
                let ds = (full_w / bw).min(88. / bh);
                let (desk_w, desk_h) = (bw * ds, bh * ds);
                let bezel = 3.;
                let screens: Vec<AnyElement> = monitors
                    .iter()
                    .enumerate()
                    .map(|(i, m)| {
                        let (mx, my) = ((m.x - min_x) as f32, (m.y - min_y) as f32);
                        let (sx, sy) = (mx * ds, my * ds);
                        let (sw, sh) = (m.width as f32 * ds - 4., m.height as f32 * ds - 4.);
                        let (iw2, ih2) = (sw - bezel * 2., sh - bezel * 2.);
                        let tint = self.sharpness_color(span[i]);
                        // The picture as a span lays it over the whole desk,
                        // seen through this screen.
                        let (pl, pt) = ((ox - mx) * ds - bezel, (oy - my) * ds - bezel);
                        let slice = thumb.clone().map(|t| {
                            div()
                                .absolute()
                                .top(px(bezel))
                                .left(px(bezel))
                                .h(px(ih2))
                                .overflow_hidden()
                                .child(
                                    div().w(px(iw2)).h(px(ih2)).relative().overflow_hidden().child(
                                        boxed(img(t), img_w * ds, img_h * ds).absolute().left(px(pl)).top(px(pt)),
                                    ),
                                )
                                .child(div().absolute().top_0().bottom_0().right_0().w(px(2.)).bg(with_alpha(c, 0.9)))
                                .with_animation(
                                    SharedString::from(format!("fit-slice-{key}-{i}")),
                                    delayed(150 + i as u64 * 140, 420),
                                    move |d, t| d.w(px(iw2 * ease_out_cubic(t))),
                                )
                        });
                        div()
                            .absolute()
                            .left(px(sx + 2.))
                            .top(px(sy + 2.))
                            .w(px(sw))
                            .h(px(sh + STAND))
                            .child(
                                div()
                                    .relative()
                                    .w(px(sw))
                                    .h(px(sh))
                                    .rounded_md()
                                    .bg(hsla(0., 0., 0.04, 1.))
                                    .children(slice)
                                    .child(div().absolute().inset_0().rounded_md().border_1().border_color(tint)),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .top(px(sh))
                                    .left(px(sw / 2. - 2.))
                                    .w(px(4.))
                                    .h(px(STAND - 4.))
                                    .bg(with_alpha(tint, 0.6)),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .top(px(sh + STAND - 4.))
                                    .left(px(sw / 2. - 14.))
                                    .w(px(28.))
                                    .h(px(3.))
                                    .rounded_full()
                                    .bg(with_alpha(tint, 0.6)),
                            )
                            .into_any_element()
                    })
                    .collect();
                // Under each stand: its name, scale and how it'll look.
                let captions: Vec<AnyElement> = monitors
                    .iter()
                    .enumerate()
                    .map(|(i, m)| {
                        let sx = (m.x - min_x) as f32 * ds;
                        let sw = m.width as f32 * ds;
                        let tint = self.sharpness_color(span[i]);
                        v_flex()
                            .absolute()
                            .left(px(sx))
                            .w(px(sw))
                            .items_center()
                            .text_size(px(9.))
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(m.name.clone()))
                            .child(div().text_color(tint).font_weight(FontWeight::SEMIBOLD).child(format!(
                                "{:.1}x  {}",
                                span[i],
                                scale_word(span[i])
                            )))
                            .into_any_element()
                    })
                    .collect();
                let desk = v_flex()
                    .items_center()
                    .gap_1()
                    .child(div().relative().w(px(desk_w)).h(px(desk_h + STAND)).children(screens))
                    .child(div().w(px(desk_w)).h(px(3.)).rounded_full().bg(with_alpha(c, 0.35)))
                    .child(div().relative().w(px(desk_w)).h(px(24.)).children(captions));

                let label = |text: &'static str| {
                    div().text_size(px(9.)).font_weight(FontWeight::SEMIBOLD).text_color(p.dim).child(text)
                };
                v_flex()
                    .w_full()
                    .gap_2()
                    .child(label("THE PICTURE  ·  WHAT A SPAN KEEPS"))
                    .child(h_flex().justify_center().child(map))
                    .child(label("ON YOUR DESK"))
                    .child(desk)
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
