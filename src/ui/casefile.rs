//! The Info side of the case file: four readouts that count themselves up,
//! the chain of custody (uploaded, by whom, from where), the fit check and
//! palette X-ray, then the tags as evidence, dropping in one by one.

use std::time::Duration;

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::details::ago;
use super::{InfoState, Muralist, compact_count, icon, pal, radius, section, with_alpha};
use crate::thumbs::Job;
use crate::wallhaven::Wallpaper;

/// A number that counts itself up to `value`.
fn readout(
    id: SharedString,
    icon_name: &'static str,
    label: &'static str,
    value: Option<f64>,
    fmt: fn(f64) -> String,
    c: Hsla,
) -> AnyElement {
    let p = pal();
    let head = h_flex()
        .gap_1()
        .text_size(px(9.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(p.dim)
        .child(icon(icon_name).size(px(10.)).text_color(c))
        .child(label);
    let card = v_flex()
        .relative()
        .gap_1()
        .p_2p5()
        .rounded_lg()
        .overflow_hidden()
        .bg(linear_gradient(160., linear_color_stop(with_alpha(c, 0.12), 0.), linear_color_stop(p.bg, 0.8)))
        .border_1()
        .border_color(with_alpha(c, 0.3))
        // A tick down the left edge, like a gauge.
        .child(div().absolute().left_0().top(px(10.)).bottom(px(10.)).w(px(2.)).rounded_full().bg(c))
        .child(head);
    // Distinct ids for waiting and counting, or the count would inherit the
    // pulse's start time and land without counting.
    let id = SharedString::from(format!("{id}-{}", if value.is_some() { "n" } else { "p" }));
    match value {
        Some(v) => card
            .child(div().text_size(px(20.)).line_height(px(22.)).font_weight(FontWeight::BOLD).with_animation(
                id,
                Animation::new(Duration::from_millis(900)).with_easing(ease_out_quint()),
                move |d, t| d.child(fmt(v * f64::from(t))),
            ))
            .into_any_element(),
        None => card
            .child(div().text_size(px(20.)).line_height(px(22.)).font_weight(FontWeight::BOLD).text_color(p.dim).child(
                div().child("--").with_animation(
                    id,
                    Animation::new(Duration::from_millis(900)).repeat().with_easing(pulsating_between(0.3, 1.)),
                    gpui_kit::Styled::opacity,
                ),
            ))
            .into_any_element(),
    }
}

impl Muralist {
    pub(super) fn render_case_info(&mut self, w: &Wallpaper, c: Hsla, cx: &mut Context<Self>) -> AnyElement {
        let p = pal();
        let info = match self.info.get(&w.id) {
            Some(InfoState::Ready(i)) => Some(i.clone()),
            _ => None,
        };
        let failed = match self.info.get(&w.id) {
            Some(InfoState::Failed(e)) => Some(e.clone()),
            _ => None,
        };
        let key = w.id.clone();

        // ---- readouts ----
        let mp = w.dims().map(|(a, b)| f64::from(a) * f64::from(b) / 1e6);
        let readouts = div()
            .grid()
            .grid_cols(2)
            .gap_2()
            .child(readout(format!("ro-mp-{key}").into(), "maximize", "MEGAPIXELS", mp, |v| format!("{v:.1}"), c))
            .child(readout(
                format!("ro-mb-{key}").into(),
                "hard-drive",
                "MEGABYTES",
                Some(super::mib(w.file_size)),
                |v| format!("{v:.1}"),
                c,
            ))
            .child(readout(
                format!("ro-views-{key}").into(),
                "eye",
                "VIEWS",
                info.as_ref().map(|i| i.views as f64),
                |v| compact_count(v.round() as u64),
                c,
            ))
            .child(readout(
                format!("ro-favs-{key}").into(),
                "heart",
                "FAVORITES",
                Some(w.favorites as f64),
                |v| compact_count(v.round() as u64),
                c,
            ));

        // ---- the route: where it came from, through whom, to here ----
        let uploader = info.as_ref().and_then(|i| i.uploader.clone());
        let avatar = uploader.as_ref().and_then(|u| {
            let url = u.avatar.get("128px").or_else(|| u.avatar.get("32px"))?.clone();
            self.thumb(format!("avatar-{}", u.username), |key| Job::Remote { key, url })
        });
        let source = info.as_ref().map(|i| i.source.clone()).filter(|s| !s.is_empty());
        let host = source.as_ref().map(|url| {
            url.split("//")
                .nth(1)
                .and_then(|r| r.split('/').next())
                .unwrap_or(url)
                .trim_start_matches("www.")
                .to_owned()
        });
        let uploaded = info.as_ref().and_then(|i| ago(&i.created_at));
        // Here: up on a screen, saved, or only seen.
        let on: Vec<String> = self
            .applied
            .desktop
            .iter()
            .chain(self.applied.lock.iter())
            .filter(|(_, path)| crate::provenance::wallhaven_id(path) == Some(w.id.as_str()))
            .map(|(m, _)| m.clone())
            .collect();
        let saved = self.provenance.records.get(&w.id).map(|r| r.saved);
        let here = if !on.is_empty() {
            (format!("on {}", on.join(", ")), "up right now".to_owned(), true)
        } else if let Some(at) = saved.and_then(|t| chrono::DateTime::from_timestamp(t, 0)) {
            ("saved".to_owned(), at.with_timezone(&chrono::Local).format("%b %-d").to_string(), true)
        } else {
            ("not yet".to_owned(), "only looked at".to_owned(), false)
        };

        struct Station {
            id: &'static str,
            label: &'static str,
            node: AnyElement,
            value: String,
            detail: String,
            lit: bool,
        }
        let glyph = |name: &'static str, lit: bool| {
            icon(name).size(px(14.)).text_color(if lit { c } else { p.dim }).into_any_element()
        };
        let stations = [
            Station {
                id: "route-origin",
                label: "ORIGIN",
                node: glyph("link", host.is_some()),
                value: host.clone().unwrap_or_else(|| "unknown".into()),
                detail: if host.is_some() { "credited source".into() } else { "not credited".into() },
                lit: host.is_some(),
            },
            Station {
                id: "route-uploader",
                label: "UPLOADER",
                node: match avatar {
                    Some(a) => radius(img(a).size_full().object_fit(ObjectFit::Cover), 14.).into_any_element(),
                    None => glyph("user", uploader.is_some()),
                },
                value: uploader.as_ref().map_or_else(|| "...".into(), |u| format!("@{}", u.username)),
                detail: "uploaded it".into(),
                lit: uploader.is_some(),
            },
            Station {
                id: "route-wallhaven",
                label: "WALLHAVEN",
                node: glyph("images", true),
                value: w.id.clone(),
                detail: uploaded.as_ref().map_or_else(|| "...".into(), |(when, _)| when.clone()),
                lit: true,
            },
            Station {
                id: "route-here",
                label: "HERE",
                node: glyph("monitor", here.2),
                value: here.0,
                detail: here.1,
                lit: here.2,
            },
        ];
        let fresh = uploaded.as_ref().is_some_and(|(_, fresh)| *fresh);
        let count = stations.len() as f32;
        const NODE: f32 = 30.;
        // The line through the nodes: lit up to the last station reached.
        let reached = stations.iter().rposition(|s| s.lit).unwrap_or(0) as f32;
        let span_of = |n: f32| relative(n / count);
        let route_line = div()
            .absolute()
            .top(px(NODE / 2. - 1.))
            .left(span_of(0.5))
            .right(span_of(0.5))
            .h(px(2.))
            .rounded_full()
            .bg(p.border_strong)
            .child(
                div()
                    .h_full()
                    .rounded_full()
                    .bg(linear_gradient(90., linear_color_stop(with_alpha(c, 0.3), 0.), linear_color_stop(c, 1.)))
                    .w(relative(reached / (count - 1.)))
                    .with_animation(
                        SharedString::from(format!("route-fill-{key}")),
                        Animation::new(Duration::from_millis(900)).with_easing(ease_out_quint()),
                        move |d, t| d.w(relative(reached / (count - 1.) * t)),
                    ),
            );
        // Packets travelling the route toward here.
        let packets = (0..3).map(|k| {
            div()
                .absolute()
                .top(px(NODE / 2. - 3.))
                .size(px(6.))
                .rounded_full()
                .bg(white())
                .shadow(super::glass::ambilight(c, 6., 1.))
                .with_animation(
                    SharedString::from(format!("route-packet-{key}-{k}")),
                    Animation::new(Duration::from_millis(2600)).repeat(),
                    move |d, t| {
                        let t = (t + k as f32 / 3.).fract();
                        // From the first node's centre to the last reached.
                        let from = 0.5 / count;
                        let to = (reached + 0.5) / count;
                        d.left(relative(from + (to - from) * t)).opacity((1. - (t * 2. - 1.).abs()) * 1.4)
                    },
                )
        });
        let columns = stations.into_iter().enumerate().map(|(i, s)| {
            let lit = s.lit;
            let wallhaven = s.id == "route-wallhaven";
            v_flex()
                .id(s.id)
                .flex_1()
                .min_w_0()
                .items_center()
                .gap_1()
                .child(
                    div()
                        .relative()
                        .size(px(NODE))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .overflow_hidden()
                        .bg(if lit { with_alpha(c, 0.18) } else { p.bg })
                        .border_1()
                        .border_color(if lit { c } else { p.border_strong })
                        .child(s.node)
                        .with_animation(
                            SharedString::from(format!("route-node-{key}-{i}")),
                            super::glass::delayed(i as u64 * 120, 320),
                            |d, t| d.opacity(super::glass::ease_out_cubic(t)),
                        ),
                )
                .child(
                    div()
                        .text_size(px(9.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(if lit { c } else { p.dim })
                        .child(s.label),
                )
                .child(
                    div()
                        .max_w_full()
                        .truncate()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(if lit { p.fg } else { p.dim })
                        .child(s.value),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .text_size(px(10.))
                        .text_color(p.dim)
                        .child(div().max_w_full().truncate().child(s.detail))
                        .when(wallhaven && fresh, |d| {
                            d.child(
                                div()
                                    .px_1()
                                    .rounded_sm()
                                    .bg(with_alpha(p.ok, 0.18))
                                    .text_size(px(9.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(p.ok)
                                    .child("NEW")
                                    .with_animation(
                                        "fresh-pulse",
                                        Animation::new(Duration::from_millis(1200))
                                            .repeat()
                                            .with_easing(pulsating_between(0.5, 1.)),
                                        |d, t| d.opacity(t),
                                    ),
                            )
                        }),
                )
                .into_any_element()
        });
        let source_url = source.clone();
        let user = uploader.as_ref().map(|u| u.username.clone());
        let custody =
            div()
                .relative()
                .child(route_line)
                .children(packets)
                .child(h_flex().items_start().children(columns))
                // Origin and uploader lead somewhere.
                .child(
                    h_flex()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .h(px(NODE))
                        .child(div().id("route-origin-hit").flex_1().h_full().when_some(source_url, |d, url| {
                            d.cursor_pointer().on_click(move |_, _, cx| cx.open_url(&url))
                        }))
                        .child(div().id("route-uploader-hit").flex_1().h_full().when_some(user, |d, name| {
                            d.cursor_pointer().on_click(
                                cx.listener(move |this, _, window, cx| this.search_for(format!("@{name}"), window, cx)),
                            )
                        }))
                        .child(div().flex_1())
                        .child(div().flex_1()),
                );

        // ---- tags ----
        let tags: AnyElement = match (&info, failed) {
            (Some(i), _) if i.tags.is_empty() => {
                div().text_xs().text_color(p.dim).child("No tags yet").into_any_element()
            }
            (Some(i), _) => {
                let tags = i.tags.clone();
                self.render_tag_groups(&tags, cx)
            }
            (None, Some(e)) => {
                let id = w.id.clone();
                h_flex()
                    .id("info-retry")
                    .gap_2()
                    .text_xs()
                    .text_color(p.dim)
                    .cursor_pointer()
                    .child(icon("circle-alert").size_3p5().text_color(p.error))
                    .child(format!("Couldn't load tags ({e}). Retry"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.load_info(id.clone(), true, cx);
                        cx.notify();
                    }))
                    .into_any_element()
            }
            (None, None) => h_flex()
                .flex_wrap()
                .gap_1p5()
                .children([64., 92., 52., 104., 70.].into_iter().enumerate().map(|(i, w)| {
                    div()
                        .relative()
                        .overflow_hidden()
                        .w(px(w))
                        .h(px(26.))
                        .rounded_md()
                        .bg(p.surface2)
                        .child(super::shimmer(SharedString::from(format!("tag-sk-{i}"))))
                }))
                .into_any_element(),
        };

        let fit = self.render_fit_check(w, c, cx);
        let palette = self.render_xray_palette(cx);
        v_flex()
            .gap_5()
            .child(readouts)
            .child(section("Where it has been", p).child(custody))
            .children(fit)
            .children(palette)
            .child(section("Evidence", p).child(tags))
            .into_any_element()
    }
}
