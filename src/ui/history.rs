//! History as a timeline: every set you've put up, by day, hanging off a
//! rail with its time and a dot in its own color, its card tinted the same
//! and showing your real desk. It says which set is up now and how long each
//! one stayed. The sidebar holds a calendar where each day is painted in
//! the color of what you set that day (click one to see just it) and the
//! stats: how many sets, desktop against lock, the longest-lived set and the
//! colors you wear most.

use crate::config::Surface;
use crate::wallhaven::Purity;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use chrono::{Datelike as _, Local, NaiveDate, TimeZone as _};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{GRID_PAD, Muralist, Target, TargetMode, boxed, glass, icon, kit, pal, radius, side_card_with, with_alpha};
use crate::config::HistoryEntry;

const MINI_W: f32 = 340.;
const MINI_H: f32 = 74.;
/// Width of the time column and the rail beside it.
const TIME_W: f32 = 58.;
const RAIL_W: f32 = 22.;

fn local_date(at: i64) -> Option<NaiveDate> {
    Local.timestamp_opt(at, 0).single().map(|t| t.date_naive())
}

fn day_label(day: NaiveDate) -> String {
    let today = Local::now().date_naive();
    match (today - day).num_days() {
        0 => "Today".into(),
        1 => "Yesterday".into(),
        2..=6 => day.format("%A").to_string(),
        _ => day.format("%B %-d, %Y").to_string(),
    }
}

fn time_label(at: i64) -> String {
    Local.timestamp_opt(at, 0).single().map(|t| t.format("%H:%M").to_string()).unwrap_or_default()
}

/// "moments", "42m", "3h 12m", "2d 4h".
fn span_label(secs: i64) -> String {
    let m = secs / 60;
    match m {
        0 => "moments".into(),
        1..=59 => format!("{m}m"),
        60..=1439 => format!("{}h {}m", m / 60, m % 60),
        _ => format!("{}d {}h", m / 1440, (m % 1440) / 60),
    }
}

/// How long each entry stayed up: until the next entry for the same
/// surface. The newest per surface has none (it may still be up).
fn stays(history: &[HistoryEntry]) -> HashMap<usize, i64> {
    let mut out = HashMap::new();
    let mut next_at: HashMap<Surface, i64> = HashMap::new();
    for (ix, e) in history.iter().enumerate().rev() {
        if let Some(n) = next_at.get(&e.target) {
            out.insert(ix, n - e.at);
        }
        next_at.insert(e.target, e.at);
    }
    out
}

impl Muralist {
    /// An entry's own color: the vivid tone of its first image.
    fn entry_color(&mut self, e: &HistoryEntry) -> Option<Hsla> {
        // A missing file just never gets a thumbnail, so no disk checks here.
        e.images.values().find_map(|path| self.path_glow(path))
    }

    /// Runs with the history lent out of `applied` (rendering never reads
    /// it from there), so the frame doesn't clone hundreds of entries.
    fn with_history<R>(&mut self, f: impl FnOnce(&mut Self, &[HistoryEntry]) -> R) -> R {
        let history = std::mem::take(&mut self.applied.history);
        let out = f(self, &history);
        self.applied.history = history;
        out
    }

    pub(super) fn render_history(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.with_history(|this, history| this.render_history_of(history, cx))
    }

    fn render_history_of(&mut self, history: &[HistoryEntry], cx: &mut Context<Self>) -> AnyElement {
        let p = pal();
        if history.is_empty() {
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

        let stays = stays(history);
        // Newest first, grouped by day, narrowed to a picked day.
        let mut days: Vec<(NaiveDate, Vec<usize>)> = Vec::new();
        for ix in (0..history.len()).rev() {
            let Some(day) = local_date(history[ix].at) else { continue };
            if self.history_day.is_some_and(|d| d != day) {
                continue;
            }
            match days.last_mut() {
                Some((d, list)) if *d == day => list.push(ix),
                _ => days.push((day, vec![ix])),
            }
        }

        let deal = self.dealt.0;
        let mut row_n = 0;
        let mut sections: Vec<AnyElement> = Vec::new();
        for (day, list) in days {
            let colors: Vec<Hsla> = list.iter().filter_map(|&ix| self.entry_color(&history[ix])).collect();
            let header = h_flex()
                .gap_3()
                .pb_2()
                .child(div().w(px(TIME_W + RAIL_W)).flex_none())
                .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child(day_label(day)))
                .child(div().text_sm().text_color(p.dim).child(day.format("%a %-d %b").to_string()))
                .child(div().flex_1())
                .child(
                    h_flex()
                        .gap_1()
                        .children(colors.iter().take(10).map(|c| div().w(px(14.)).h(px(6.)).rounded_full().bg(*c))),
                )
                .child(div().text_xs().text_color(p.dim).child(format!(
                    "{} set{}",
                    list.len(),
                    if list.len() == 1 { "" } else { "s" }
                )));
            let last = list.len() - 1;
            let rows: Vec<AnyElement> = list
                .into_iter()
                .enumerate()
                .map(|(i, ix)| {
                    let el = self.timeline_row(ix, &history[ix], stays.get(&ix).copied(), i == 0, i == last, cx);
                    row_n += 1;
                    glass::stagger(SharedString::from(format!("hist-{ix}-{deal}")), row_n.min(14), el)
                        .into_any_element()
                })
                .collect();
            sections.push(v_flex().child(header).children(rows).into_any_element());
        }

        div()
            .id("history")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(v_flex().p(px(GRID_PAD)).gap_6().children(sections))
            .into_any_element()
    }

    fn timeline_row(
        &mut self,
        ix: usize,
        entry: &HistoryEntry,
        stayed: Option<i64>,
        first: bool,
        last: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = pal();
        let hovered = self.history_hover == Some(ix);
        let is_lock = entry.target == Surface::Lock;
        let c = self.entry_color(entry).unwrap_or(p.accent);
        let applied = if is_lock { &self.applied.lock } else { &self.applied.desktop };
        let up_now = stayed.is_none() && !entry.images.is_empty() && entry.images == *applied;
        let peeking_this = hovered && self.peek.is_some();
        let mini = self.mini_layout(&entry.images);
        let purities: Vec<Purity> =
            entry.images.values().filter_map(|path| self.provenance.of(path).map(|r| r.wallpaper.purity)).collect();

        let stay_text = match stayed {
            Some(s) => format!("stayed {}", span_label(s)),
            None if up_now => format!("up now  ·  for {}", span_label(chrono::Utc::now().timestamp() - entry.at)),
            None => "replaced since".into(),
        };

        // The rail: a line through every row, broken only at the ends of a
        // day, and a dot in the set's own color.
        let rail = div()
            .relative()
            .flex_none()
            .w(px(RAIL_W))
            .self_stretch()
            .child(
                div()
                    .absolute()
                    .left(px(RAIL_W / 2. - 1.))
                    .w(px(2.))
                    .top(px(if first { 30. } else { 0. }))
                    .bottom_0()
                    .when(last, |d| d.h(px(30.)))
                    .bg(with_alpha(p.border_strong, 0.8)),
            )
            .child(
                div()
                    .absolute()
                    .left(px(RAIL_W / 2. - 7.))
                    .top(px(23.))
                    .size(px(14.))
                    .rounded_full()
                    .bg(c)
                    .border_2()
                    .border_color(p.bg)
                    .when(up_now, gpui_kit::Styled::shadow_lg),
            )
            .when(up_now, |d| {
                d.child(
                    div()
                        .absolute()
                        .left(px(RAIL_W / 2. - 11.))
                        .top(px(19.))
                        .size(px(22.))
                        .rounded_full()
                        .border_1()
                        .border_color(c)
                        .with_animation(
                            SharedString::from(format!("hist-now-{ix}")),
                            Animation::new(std::time::Duration::from_millis(1800))
                                .repeat()
                                .with_easing(pulsating_between(0.2, 1.)),
                            gpui_kit::Styled::opacity,
                        ),
                )
            });

        let card = h_flex()
            .id(("hist", ix))
            .flex_1()
            .min_w_0()
            .gap_4()
            .p_3()
            .rounded_xl()
            .bg(linear_gradient(
                90.,
                linear_color_stop(with_alpha(c, if hovered { 0.2 } else { 0.12 }), 0.),
                linear_color_stop(with_alpha(p.surface, 0.9), 0.6),
            ))
            .border_1()
            .border_color(if hovered || peeking_this { with_alpha(c, 0.8) } else { with_alpha(c, 0.3) })
            .child(mini)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1p5()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                h_flex()
                                    .gap_1()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_full()
                                    .bg(with_alpha(c, 0.18))
                                    .text_xs()
                                    .text_color(p.fg)
                                    .child(icon(if is_lock { "lock" } else { "monitor" }).size_3())
                                    .child(if is_lock { "Lock screen" } else { "Desktop" }),
                            )
                            .when(up_now, |d| {
                                d.child(
                                    div()
                                        .px_2()
                                        .py_0p5()
                                        .rounded_full()
                                        .bg(c)
                                        .text_xs()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(super::on_accent(c))
                                        .child("Up now"),
                                )
                            }),
                    )
                    .child(div().text_xs().text_color(p.muted).child(stay_text))
                    .child(h_flex().gap_1().children(
                        purities.iter().map(|pu| div().size(px(6.)).rounded_full().bg(super::purity_color(*pu))),
                    )),
            )
            .child(
                v_flex()
                    .flex_none()
                    .items_end()
                    .gap_1p5()
                    .when(!hovered, gpui_kit::Styled::invisible)
                    .child(
                        kit::Btn::new(("hist-set", ix))
                            .label("Set again")
                            .icon("rotate-ccw")
                            .primary()
                            .sm()
                            .tooltip("Put this set back up")
                            .on_click(cx.listener(move |this, _, _, cx| this.restore_history(ix, cx))),
                    )
                    .when(!is_lock, |d| {
                        d.child(
                            h_flex()
                                .gap_1()
                                .text_xs()
                                .text_color(p.dim)
                                .child(super::keycap("Space", p))
                                .child("hold to peek"),
                        )
                    }),
            )
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.history_hover = Some(ix);
                } else if this.history_hover == Some(ix) {
                    this.history_hover = None;
                }
                cx.notify();
            }));

        h_flex()
            .items_start()
            .child(
                v_flex()
                    .flex_none()
                    .w(px(TIME_W))
                    .pt(px(20.))
                    .items_end()
                    .pr_2()
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(time_label(entry.at))),
            )
            .child(rail)
            .child(div().flex_1().min_w_0().py_1p5().pl_2().child(card))
            .into_any_element()
    }

    /// The monitor layout in miniature, each screen showing its image.
    fn mini_layout(&mut self, images: &BTreeMap<String, PathBuf>) -> Div {
        let p = pal();
        let monitors = self.monitors.clone();
        if monitors.is_empty() {
            return div();
        }
        let (min_x, min_y, max_x, max_y) = crate::monitors::extent(&monitors);
        let scale = (MINI_W / (max_x - min_x) as f32).min(MINI_H / (max_y - min_y) as f32);
        let gap = 4.;
        let screens: Vec<Div> = monitors
            .iter()
            .map(|m| {
                let path = images.get(&m.name).cloned();
                let thumb = path.as_deref().and_then(|p| self.local_thumb(p));
                // What it was when it was saved, even if it's only a span slice now.
                let purity = path.as_deref().and_then(|p| self.provenance.of(p)).map(|r| r.wallpaper.purity);
                let (tw, th) = (m.width as f32 * scale - gap, m.height as f32 * scale - gap);
                div()
                    .absolute()
                    .left(px((m.x - min_x) as f32 * scale + gap / 2.))
                    .top(px((m.y - min_y) as f32 * scale + gap / 2.))
                    .w(px(tw))
                    .h(px(th))
                    .rounded_md()
                    .bg(p.bg)
                    .map(|d| match thumb {
                        Some(t) => d.child(radius(boxed(img(t), tw, th).object_fit(ObjectFit::Cover), 6.)),
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
                    .child(div().absolute().inset_0().rounded_md().border_1().border_color(match purity {
                        Some(pu) => with_alpha(super::purity_color(pu), 0.75),
                        None => p.border,
                    }))
            })
            .collect();
        div()
            .relative()
            .flex_none()
            .w(px((max_x - min_x) as f32 * scale))
            .h(px((max_y - min_y) as f32 * scale))
            .children(screens)
    }

    /// The History tab's sidebar: the color calendar and the stats.
    pub(super) fn render_history_side(&mut self, cx: &mut Context<Self>) -> Div {
        self.with_history(|this, history| this.render_history_side_of(history, cx))
    }

    fn render_history_side_of(&mut self, history: &[HistoryEntry], cx: &mut Context<Self>) -> Div {
        let p = pal();
        let today = Local::now().date_naive();
        let (year, month) = self.history_month.unwrap_or((today.year(), today.month()));

        // Each day's color is that of the last set put up that day.
        let mut by_day: BTreeMap<NaiveDate, (usize, Option<Hsla>)> = BTreeMap::new();
        for e in history {
            let Some(day) = local_date(e.at) else { continue };
            let c = self.entry_color(e);
            let slot = by_day.entry(day).or_insert((0, None));
            slot.0 += 1;
            if c.is_some() {
                slot.1 = c;
            }
        }

        let first = NaiveDate::from_ymd_opt(year, month, 1).unwrap_or(today);
        let next_month = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
        let prev_month = if month == 1 { (year - 1, 12) } else { (year, month - 1) };
        let days_in =
            NaiveDate::from_ymd_opt(next_month.0, next_month.1, 1).map_or(30, |d| (d - first).num_days() as u32);
        let lead = first.weekday().num_days_from_monday() as usize;
        let picked = self.history_day;

        let weekdays = ["M", "T", "W", "T", "F", "S", "S"]
            .into_iter()
            .map(|w| div().h(px(16.)).flex().justify_center().text_xs().text_color(p.dim).child(w).into_any_element());
        let blanks = (0..lead).map(|_| div().into_any_element());
        let cells = (1..=days_in).filter_map(|d| NaiveDate::from_ymd_opt(year, month, d)).map(|day| {
            let info = by_day.get(&day).copied();
            let on = picked == Some(day);
            let is_today = day == today;
            let n = info.map_or(0, |i| i.0);
            let c = info.and_then(|i| i.1).unwrap_or(p.accent);
            div()
                .id(("cal", day.num_days_from_ce() as usize))
                .h(px(28.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_md()
                .text_xs()
                .map(|d| {
                    if n > 0 {
                        // Busier days glow brighter.
                        d.bg(with_alpha(c, (0.3 + 0.15 * n as f32).min(0.95)))
                            .text_color(white())
                            .cursor_pointer()
                            .hover(move |s| s.bg(c))
                    } else {
                        d.bg(with_alpha(p.surface2, 0.5)).text_color(p.dim)
                    }
                })
                .when(on, |d| d.border_2().border_color(p.fg))
                .when(is_today && !on, |d| d.border_1().border_color(with_alpha(p.fg, 0.5)))
                .child(day.day().to_string())
                .when(n > 0, |d| {
                    d.on_click(cx.listener(move |this, _, _, cx| {
                        this.history_day = if this.history_day == Some(day) { None } else { Some(day) };
                        this.deal();
                        cx.notify();
                    }))
                })
                .into_any_element()
        });

        let month_name = first.format("%B %Y").to_string();
        let calendar = side_card_with("calendar", &month_name, p, |h| {
            h.child(kit::Btn::new("cal-prev").icon("chevron-left").ghost().sm().on_click(cx.listener(
                move |this, _, _, cx| {
                    this.history_month = Some(prev_month);
                    cx.notify();
                },
            )))
            .child(kit::Btn::new("cal-next").icon("chevron-right").ghost().sm().on_click(cx.listener(
                move |this, _, _, cx| {
                    this.history_month = Some(next_month);
                    cx.notify();
                },
            )))
        })
        .child(div().grid().grid_cols(7).gap_1().children(weekdays).children(blanks).children(cells))
        .when_some(picked, |d, day| {
            d.child(
                h_flex()
                    .gap_2()
                    .text_xs()
                    .text_color(p.muted)
                    .child(format!("Showing {}", day.format("%a %-d %b")))
                    .child(div().flex_1())
                    .child(kit::Btn::new("cal-clear").label("All days").ghost().sm().on_click(cx.listener(
                        |this, _, _, cx| {
                            this.history_day = None;
                            this.deal();
                            cx.notify();
                        },
                    ))),
            )
        });

        // Stats.
        let total = history.len();
        let locks = history.iter().filter(|e| e.target == Surface::Lock).count();
        let stays = stays(history);
        let longest = stays.iter().max_by_key(|(_, s)| **s).map(|(ix, s)| (*ix, *s));
        let longest_thumb =
            longest.and_then(|(ix, _)| history[ix].images.values().find_map(|path| self.local_thumb(path)));
        let mut worn: Vec<Hsla> = history.iter().rev().filter_map(|e| self.entry_color(e)).collect();
        worn.dedup_by(|a, b| (a.h - b.h).abs() < 0.03 && (a.l - b.l).abs() < 0.08);
        let stat = |label: &str, value: String| {
            v_flex()
                .flex_1()
                .gap_0p5()
                .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child(value))
                .child(div().text_xs().text_color(p.dim).child(label.to_owned()))
        };
        let stats = side_card_with("chart-column", "Stats", p, |h| h)
            .child(
                h_flex()
                    .child(stat("sets", total.to_string()))
                    .child(stat("desktop", (total - locks).to_string()))
                    .child(stat("lock", locks.to_string())),
            )
            .when_some(longest, |d, (_, secs)| {
                d.child(
                    h_flex()
                        .gap_2p5()
                        .child(
                            div()
                                .flex_none()
                                .w(px(56.))
                                .h(px(34.))
                                .rounded_md()
                                .bg(p.bg)
                                .when_some(longest_thumb, |d, t| {
                                    d.child(radius(boxed(img(t), 56., 34.).object_fit(ObjectFit::Cover), 6.))
                                }),
                        )
                        .child(
                            v_flex()
                                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(span_label(secs)))
                                .child(div().text_xs().text_color(p.dim).child("longest any set stayed")),
                        ),
                )
            })
            .when(!worn.is_empty(), |d| {
                d.child(
                    v_flex().gap_1p5().child(div().text_xs().text_color(p.dim).child("Colors you wear most")).child(
                        h_flex()
                            .h(px(10.))
                            .rounded_full()
                            .overflow_hidden()
                            .children(worn.into_iter().take(12).map(|c| div().flex_1().h_full().bg(c))),
                    ),
                )
            });

        v_flex().gap_3().child(calendar).child(stats)
    }

    /// Puts a past set back up, there and then.
    fn restore_history(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(entry) = self.applied.history.get(ix).cloned() else {
            return;
        };
        let images: BTreeMap<String, PathBuf> = entry.images.into_iter().filter(|(_, path)| path.is_file()).collect();
        if images.is_empty() {
            self.set_status("Those files are gone from disk", true, cx);
            return;
        }
        let target = if entry.target == Surface::Lock {
            self.lock = images;
            self.target = TargetMode::Lock;
            Target::Lock
        } else {
            self.desktop = images;
            self.target = TargetMode::Desktop;
            Target::Desktop
        };
        self.apply(vec![target], cx);
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
        (entry.target == Surface::Desktop)
            .then(|| entry.images.iter().filter(|(_, p)| p.is_file()).map(|(k, v)| (k.clone(), v.clone())).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::{span_label, stays};
    use crate::config::HistoryEntry;
    use crate::config::Surface;
    use std::collections::BTreeMap;

    fn entry(at: i64, target: Surface) -> HistoryEntry {
        HistoryEntry { at, target, images: BTreeMap::new() }
    }

    #[test]
    fn a_set_stays_until_the_next_one_for_the_same_surface() {
        let h = [
            entry(0, Surface::Desktop),
            entry(100, Surface::Lock),
            entry(600, Surface::Desktop),
            entry(4000, Surface::Desktop),
        ];
        let s = stays(&h);
        assert_eq!(s.get(&0), Some(&600));
        assert_eq!(s.get(&2), Some(&3400));
        // Newest per surface: no end yet.
        assert_eq!(s.get(&1), None);
        assert_eq!(s.get(&3), None);
    }

    #[test]
    fn spans_read_naturally() {
        assert_eq!(span_label(20), "moments");
        assert_eq!(span_label(42 * 60), "42m");
        assert_eq!(span_label(3 * 3600 + 12 * 60), "3h 12m");
        assert_eq!(span_label(2 * 86400 + 4 * 3600), "2d 4h");
    }
}
