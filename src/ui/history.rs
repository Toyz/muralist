//! Time machine: every set of wallpapers you've applied, by day. Restore or
//! re-apply any of them, or hover one and hold Space to peek it for real.

use std::collections::BTreeMap;
use std::path::PathBuf;

use chrono::{Local, TimeZone as _};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{BtnKind, GRID_PAD, Muralist, Target, TargetMode, icon, pal, radius, section, small_button, with_alpha};
use crate::config::HistoryEntry;

const MINI_W: f32 = 300.;
const MINI_H: f32 = 64.;

fn day_label(at: i64) -> String {
    let Some(when) = Local.timestamp_opt(at, 0).single() else {
        return "Unknown date".into();
    };
    let today = Local::now().date_naive();
    match (today - when.date_naive()).num_days() {
        0 => "Today".into(),
        1 => "Yesterday".into(),
        2..=6 => when.format("%A").to_string(),
        _ => when.format("%B %-d, %Y").to_string(),
    }
}

fn time_label(at: i64) -> String {
    Local.timestamp_opt(at, 0).single().map(|t| t.format("%H:%M").to_string()).unwrap_or_default()
}

impl Muralist {
    pub(super) fn render_history(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = pal();
        let entries: Vec<(usize, HistoryEntry)> = self.applied.history.iter().cloned().enumerate().rev().collect();

        if entries.is_empty() {
            return v_flex()
                .flex_1()
                .items_center()
                .pt(px(140.))
                .gap_3()
                .child(
                    div()
                        .size(px(64.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(with_alpha(p.accent, 0.12))
                        .border_1()
                        .border_color(with_alpha(p.accent, 0.35))
                        .child(icon("calendar").size(px(26.)).text_color(p.accent)),
                )
                .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child("No history yet"))
                .child(div().text_sm().text_color(p.dim).child("Every set of wallpapers you apply lands here."))
                .into_any_element();
        }

        // Group consecutive entries by local day.
        let mut groups: Vec<(String, Vec<(usize, HistoryEntry)>)> = Vec::new();
        for (ix, entry) in entries {
            let label = day_label(entry.at);
            match groups.last_mut() {
                Some((l, list)) if *l == label => list.push((ix, entry)),
                _ => groups.push((label, vec![(ix, entry)])),
            }
        }

        let groups: Vec<AnyElement> = groups
            .into_iter()
            .map(|(label, list)| {
                let cards: Vec<AnyElement> = list.into_iter().map(|(ix, e)| self.history_card(ix, e, cx)).collect();
                section(&label, p).child(v_flex().gap_2().children(cards)).into_any_element()
            })
            .collect();

        div()
            .id("history")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(v_flex().p(px(GRID_PAD)).gap_6().children(groups))
            .into_any_element()
    }

    fn history_card(&mut self, ix: usize, entry: HistoryEntry, cx: &mut Context<Self>) -> AnyElement {
        let p = pal();
        let hovered = self.history_hover == Some(ix);
        let is_lock = entry.target == "lock";
        let mini = self.mini_layout(&entry.images);
        let peeking_this = hovered && self.peek.is_some();

        h_flex()
            .id(("hist", ix))
            .gap_5()
            .px_4()
            .py_3()
            .rounded_xl()
            .bg(p.surface)
            .border_1()
            .border_color(if hovered { with_alpha(p.accent, 0.6) } else { p.border })
            .when(peeking_this, |d| d.border_color(p.accent))
            .child(
                v_flex()
                    .w(px(96.))
                    .gap_1()
                    .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child(time_label(entry.at)))
                    .child(
                        div()
                            .px_2()
                            .py_0p5()
                            .rounded_full()
                            .bg(p.surface2)
                            .text_xs()
                            .text_color(if is_lock { p.accent } else { p.muted })
                            .child(if is_lock { "Lock screen" } else { "Desktop" }),
                    ),
            )
            .child(mini)
            .child(
                v_flex()
                    .flex_1()
                    .items_end()
                    .gap_2()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                small_button(("hist-stage", ix), "Restore", BtnKind::Secondary)
                                    .on_click(cx.listener(move |this, _, _, cx| this.restore_history(ix, false, cx))),
                            )
                            .child(
                                small_button(("hist-apply", ix), "Apply", BtnKind::Primary)
                                    .on_click(cx.listener(move |this, _, _, cx| this.restore_history(ix, true, cx))),
                            ),
                    )
                    .when(hovered && !is_lock, |d| {
                        d.child(div().text_xs().text_color(p.dim).child("Hold Space to peek"))
                    }),
            )
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.history_hover = Some(ix);
                } else if this.history_hover == Some(ix) {
                    this.history_hover = None;
                }
                cx.notify();
            }))
            .into_any_element()
    }

    /// The monitor layout in miniature, each screen showing its entry image.
    fn mini_layout(&mut self, images: &BTreeMap<String, PathBuf>) -> Div {
        let p = pal();
        let monitors = self.monitors.clone();
        if monitors.is_empty() {
            return div();
        }
        let min_x = monitors.iter().map(|m| m.x).min().unwrap();
        let min_y = monitors.iter().map(|m| m.y).min().unwrap();
        let max_x = monitors.iter().map(|m| m.x + m.width as i32).max().unwrap();
        let max_y = monitors.iter().map(|m| m.y + m.height as i32).max().unwrap();
        let scale = (MINI_W / (max_x - min_x) as f32).min(MINI_H / (max_y - min_y) as f32);
        let gap = 4.;
        let screens: Vec<Div> = monitors
            .iter()
            .map(|m| {
                let path = images.get(&m.name).filter(|p| p.is_file()).cloned();
                let thumb = path.as_deref().and_then(|p| self.local_thumb(p));
                div()
                    .absolute()
                    .left(px((m.x - min_x) as f32 * scale + gap / 2.))
                    .top(px((m.y - min_y) as f32 * scale + gap / 2.))
                    .w(px(m.width as f32 * scale - gap))
                    .h(px(m.height as f32 * scale - gap))
                    .rounded_md()
                    .overflow_hidden()
                    .bg(p.bg)
                    .border_1()
                    .border_color(p.border)
                    .map(|d| match thumb {
                        Some(t) => d.child(radius(img(t).size_full().object_fit(ObjectFit::Cover), 5.)),
                        None if images.contains_key(&m.name) => d.child(
                            div()
                                .size_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_xs()
                                .text_color(p.dim)
                                .child("gone"),
                        ),
                        None => d,
                    })
            })
            .collect();
        div()
            .relative()
            .flex_none()
            .w(px((max_x - min_x) as f32 * scale))
            .h(px((max_y - min_y) as f32 * scale))
            .children(screens)
    }

    fn restore_history(&mut self, ix: usize, apply: bool, cx: &mut Context<Self>) {
        let Some(entry) = self.applied.history.get(ix).cloned() else {
            return;
        };
        let images: BTreeMap<String, PathBuf> = entry.images.into_iter().filter(|(_, path)| path.is_file()).collect();
        if images.is_empty() {
            self.set_status("Those files are gone from disk", true, cx);
            return;
        }
        let target = if entry.target == "lock" {
            self.lock = images;
            self.target = TargetMode::Lock;
            Target::Lock
        } else {
            self.desktop = images;
            self.target = TargetMode::Desktop;
            Target::Desktop
        };
        if apply {
            self.apply(vec![target], cx);
        } else {
            self.set_status("Restored, press Apply to set it", false, cx);
        }
    }

    pub(super) fn clear_history(&mut self, cx: &mut Context<Self>) {
        self.applied.history.clear();
        if let Err(e) = self.applied.save() {
            eprintln!("saving state: {e:#}");
        }
        cx.notify();
    }

    /// The hovered history entry's desktop images, for Space-peek.
    pub(super) fn history_peek_map(&self) -> Option<BTreeMap<String, PathBuf>> {
        let entry = self.applied.history.get(self.history_hover?)?;
        (entry.target == "desktop")
            .then(|| entry.images.iter().filter(|(_, p)| p.is_file()).map(|(k, v)| (k.clone(), v.clone())).collect())
    }
}
