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
        let mp =
            w.resolution.split_once('x').and_then(|(a, b)| Some(a.parse::<f64>().ok()? * b.parse::<f64>().ok()? / 1e6));
        let readouts = div()
            .grid()
            .grid_cols(2)
            .gap_2()
            .child(readout(format!("ro-mp-{key}").into(), "maximize", "MEGAPIXELS", mp, |v| format!("{v:.1}"), c))
            .child(readout(
                format!("ro-mb-{key}").into(),
                "hard-drive",
                "MEGABYTES",
                Some(w.file_size as f64 / 1_048_576.),
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

        // ---- chain of custody: when, who, where from ----
        let uploader = info.as_ref().and_then(|i| i.uploader.clone());
        let avatar = uploader.as_ref().and_then(|u| {
            let url = u.avatar.get("32px")?.clone();
            self.thumb(format!("avatar-{}", u.username), |key| Job::Remote { key, url })
        });
        let link = |id: &'static str, icon_name: &'static str, label: &'static str, value: AnyElement, last: bool| {
            h_flex()
                .id(id)
                .gap_2p5()
                .items_start()
                .child(
                    v_flex()
                        .flex_none()
                        .w(px(18.))
                        .items_center()
                        .child(
                            div()
                                .size(px(18.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded_full()
                                .border_1()
                                .border_color(c)
                                .bg(with_alpha(c, 0.15))
                                .child(icon(icon_name).size(px(10.)).text_color(c)),
                        )
                        .when(!last, |d| d.child(div().w(px(1.)).h(px(18.)).bg(with_alpha(c, 0.4)))),
                )
                .child(
                    v_flex()
                        .min_w_0()
                        .child(div().text_size(px(9.)).font_weight(FontWeight::SEMIBOLD).text_color(p.dim).child(label))
                        .child(div().text_sm().truncate().child(value)),
                )
        };
        let uploaded: AnyElement = match info.as_ref().and_then(|i| ago(&i.created_at)) {
            Some((when, fresh)) => h_flex()
                .gap_1p5()
                .child(when)
                .when(fresh, |d| {
                    d.child(
                        div()
                            .px_1p5()
                            .rounded_sm()
                            .bg(with_alpha(p.ok, 0.18))
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.ok)
                            .child("NEW")
                            .with_animation(
                                "fresh-pulse",
                                Animation::new(Duration::from_millis(1200))
                                    .repeat()
                                    .with_easing(pulsating_between(0.5, 1.)),
                                gpui_kit::Styled::opacity,
                            ),
                    )
                })
                .into_any_element(),
            None => div().child("...").into_any_element(),
        };
        let by: AnyElement = match uploader {
            Some(u) => {
                let name = u.username.clone();
                h_flex()
                    .id("uploader")
                    .gap_1p5()
                    .cursor_pointer()
                    .hover(move |s| s.text_color(c))
                    .child(
                        div()
                            .size(px(18.))
                            .flex_none()
                            .rounded_full()
                            .border_1()
                            .border_color(c)
                            .overflow_hidden()
                            .bg(p.elevated)
                            .when_some(avatar, |d, a| {
                                d.child(radius(img(a).size_full().object_fit(ObjectFit::Cover), 9.))
                            }),
                    )
                    .child(div().truncate().child(format!("@{}", u.username)))
                    .on_click(cx.listener(move |this, _, window, cx| this.search_for(format!("@{name}"), window, cx)))
                    .into_any_element()
            }
            None => div().child("...").into_any_element(),
        };
        let source: AnyElement = match info.as_ref().map(|i| i.source.clone()).filter(|s| !s.is_empty()) {
            Some(url) => {
                let host = url
                    .split("//")
                    .nth(1)
                    .and_then(|r| r.split('/').next())
                    .unwrap_or(&url)
                    .trim_start_matches("www.")
                    .to_owned();
                h_flex()
                    .id("source")
                    .gap_1()
                    .cursor_pointer()
                    .hover(move |s| s.text_color(c))
                    .child(div().truncate().child(host))
                    .child(icon("external-link").size_3())
                    .on_click(move |_, _, cx| cx.open_url(&url))
                    .into_any_element()
            }
            None => div().text_color(p.dim).child("unknown").into_any_element(),
        };
        let custody = v_flex()
            .child(link("cc-up", "calendar", "UPLOADED", uploaded, false))
            .child(link("cc-by", "user", "BY", by, false))
            .child(link("cc-src", "link", "SOURCE", source, true));

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

        let fit = self.render_fit_check(w);
        let palette = self.render_xray_palette(cx);
        v_flex()
            .gap_5()
            .child(readouts)
            .child(section("Chain of custody", p).child(custody))
            .children(fit)
            .children(palette)
            .child(section("Evidence", p).child(tags))
            .into_any_element()
    }
}
