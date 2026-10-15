//! The Library, by what its files are. Each file's provenance record gives
//! its purity, category and tags; the sidebar filters by them, a funnel
//! under the bar shows each filter narrowing the count, and the cards deal
//! back in whenever the filter changes. While files are being identified,
//! the cards themselves show it: waiting ones rest in grayscale, the one
//! being looked up has a scan beam sweeping down it, and when its record
//! lands its purity edge draws itself around the card and its tags burst up.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::{Duration, Instant};

use gpui_kit::component::input::Input;
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{Item, Muralist, Pal, icon, kit, opt, opt_grid, pal, purity_color, side_card_with, with_alpha};
use crate::provenance::wallhaven_id;

/// Tags each category shows before its "+N".
const TAGS_PER_GROUP: usize = 6;
/// Categories shown before "Show all".
const GROUPS_SHOWN: usize = 6;
/// How long a freshly identified card plays its arrival.
const LANDED: Duration = Duration::from_millis(2400);
/// How long the grid deals back in after the filter changes.
const DEAL: Duration = Duration::from_millis(900);

#[derive(Default)]
pub(super) struct LibFilter {
    pub purity: BTreeSet<String>,
    pub category: BTreeSet<String>,
    /// Picked tags, in the order they were picked (for the funnel).
    pub tags: Vec<u64>,
    /// Tag categories expanded past their first few.
    pub open_groups: BTreeSet<String>,
    pub all_groups: bool,
    /// Bumped on every change, so the cards deal in again.
    pub generation: u64,
    pub changed: Option<Instant>,
}

impl LibFilter {
    pub fn active(&self) -> bool {
        !self.purity.is_empty() || !self.category.is_empty() || !self.tags.is_empty()
    }
}

/// Where a card is in identification.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Scan {
    None,
    /// Queued: rests in grayscale.
    Waiting,
    /// Being looked up right now.
    Scanning,
    /// Its record just landed.
    Landed(Instant),
}

/// One stage of the funnel.
#[derive(Clone)]
enum Stage {
    Purity(String),
    Category(String),
    Tag(u64, String),
}

const PURITIES: [(&str, &str); 3] = [("sfw", "SFW"), ("sketchy", "Sketchy"), ("nsfw", "NSFW")];
const CATEGORIES: [(&str, &str); 3] = [("general", "General"), ("anime", "Anime"), ("people", "People")];

fn purity_label(p: &str) -> &'static str {
    PURITIES.iter().find(|(k, _)| *k == p).map_or("?", |(_, l)| l)
}

fn category_label(c: &str) -> &'static str {
    CATEGORIES.iter().find(|(k, _)| *k == c).map_or("?", |(_, l)| l)
}

impl Muralist {
    // ---- filtering ----------------------------------------------------------

    fn passes(&self, path: &std::path::Path, stages: &[Stage]) -> bool {
        if stages.is_empty() {
            return true;
        }
        // Unidentified files can't be said to match anything.
        let Some(r) = self.provenance.of(path) else { return false };
        let w = &r.wallpaper;
        // Purities and categories match any picked; tags match all.
        let purities: Vec<&String> =
            stages.iter().filter_map(|s| if let Stage::Purity(p) = s { Some(p) } else { None }).collect();
        let categories: Vec<&String> =
            stages.iter().filter_map(|s| if let Stage::Category(c) = s { Some(c) } else { None }).collect();
        (purities.is_empty() || purities.contains(&&w.purity))
            && (categories.is_empty() || categories.contains(&&w.category))
            && stages.iter().all(|s| match s {
                Stage::Tag(id, _) => r.tags.as_ref().is_some_and(|tags| tags.iter().any(|t| t.id == *id)),
                _ => true,
            })
    }

    /// The filter as an ordered list of stages: purity, then category, then
    /// tags in the order picked.
    fn stages(&self) -> Vec<Stage> {
        let f = &self.lib_filter;
        let tag_name = |id: u64| {
            self.provenance
                .records
                .values()
                .flat_map(|r| r.tags.iter().flatten())
                .find(|t| t.id == id)
                .map_or_else(|| format!("#{id}"), |t| t.name.clone())
        };
        f.purity
            .iter()
            .map(|p| Stage::Purity(p.clone()))
            .chain(f.category.iter().map(|c| Stage::Category(c.clone())))
            .chain(f.tags.iter().map(|&id| Stage::Tag(id, tag_name(id))))
            .collect()
    }

    pub(super) fn library_items(&self) -> Vec<Item> {
        let stages = self.stages();
        self.library.iter().filter(|p| self.passes(p, &stages)).cloned().map(Item::Local).collect()
    }

    fn touch_filter(&mut self, cx: &mut Context<Self>) {
        self.lib_filter.generation += 1;
        self.lib_filter.changed = Some(Instant::now());
        self.deal();
        cx.notify();
    }

    /// The grid's cards deal in afresh: new results, another tab or
    /// gallery, a changed filter.
    pub(super) fn deal(&mut self) {
        self.dealt = (self.dealt.0 + 1, Some(Instant::now()));
    }

    pub(super) fn clear_lib_filter(&mut self, cx: &mut Context<Self>) {
        self.lib_filter.purity.clear();
        self.lib_filter.category.clear();
        self.lib_filter.tags.clear();
        self.touch_filter(cx);
    }

    fn drop_stage(&mut self, stage: &Stage, cx: &mut Context<Self>) {
        match stage {
            Stage::Purity(p) => {
                self.lib_filter.purity.remove(p);
            }
            Stage::Category(c) => {
                self.lib_filter.category.remove(c);
            }
            Stage::Tag(id, _) => self.lib_filter.tags.retain(|t| t != id),
        }
        self.touch_filter(cx);
    }

    fn toggle_tag(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.lib_filter.tags.contains(&id) {
            self.lib_filter.tags.retain(|t| *t != id);
        } else {
            self.lib_filter.tags.push(id);
        }
        self.touch_filter(cx);
    }

    /// Wraps a Library card so the grid deals back in after a filter change,
    /// staggered across the grid.
    pub(super) fn deal_in(&self, card: AnyElement, ix: usize, key: &str) -> AnyElement {
        let (generation_n, at) = self.dealt;
        if !at.is_some_and(|t| t.elapsed() < DEAL) {
            return card;
        }
        let delay = (ix as f32 * 0.04).min(0.45);
        div()
            .child(card)
            .with_animation(
                SharedString::from(format!("deal-{generation_n}-{key}")),
                Animation::new(Duration::from_secs_f32(delay + 0.35)),
                move |d, t| {
                    let t = ((t * (delay + 0.35) - delay) / 0.35).clamp(0., 1.);
                    let e = 1. - (1. - t).powi(3);
                    d.opacity(e).mt(px(18. * (1. - e)))
                },
            )
            .into_any_element()
    }

    // ---- identification on the cards ------------------------------------------

    pub(super) fn scan_state(&self, item: &Item) -> Scan {
        let Item::Local(path) = item else { return Scan::None };
        let Some(id) = wallhaven_id(path) else { return Scan::None };
        if self.identifying_now.as_deref() == Some(id.as_str()) {
            return Scan::Scanning;
        }
        if let Some(at) = self.identified_at.get(&id).filter(|at| at.elapsed() < LANDED) {
            return Scan::Landed(*at);
        }
        let known = self.provenance.records.get(&id).is_some_and(|r| r.tags.is_some());
        if self.identifying.is_some() && !known { Scan::Waiting } else { Scan::None }
    }

    /// Layers drawn over a card for its identification state.
    pub(super) fn scan_layers(&self, scan: Scan, item: &Item, w: f32, h: f32, key: &str) -> Vec<AnyElement> {
        let p = pal();
        match scan {
            Scan::None => Vec::new(),
            Scan::Waiting => vec![
                div()
                    .absolute()
                    .inset_0()
                    .rounded_xl()
                    .bg(hsla(0., 0., 0., 0.3))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("scan-search").size_6().text_color(hsla(0., 0., 1., 0.35)))
                    .into_any_element(),
            ],
            Scan::Scanning => {
                const BAND: f32 = 34.;
                let c = p.accent;
                vec![
                    div().absolute().inset_0().rounded_xl().bg(hsla(0., 0., 0., 0.2)).into_any_element(),
                    // The beam: a soft band with a bright line through it,
                    // sweeping down the card over and over.
                    div()
                        .absolute()
                        .left(px(2.))
                        .right(px(2.))
                        .h(px(BAND))
                        .child(div().absolute().inset_0().bg(linear_gradient(
                            180.,
                            linear_color_stop(with_alpha(c, 0.), 0.),
                            linear_color_stop(with_alpha(c, 0.4), 1.),
                        )))
                        .child(div().absolute().left_0().right_0().bottom_0().h(px(2.)).bg(with_alpha(c, 0.95)))
                        .with_animation(
                            SharedString::from(format!("scan-{key}")),
                            Animation::new(Duration::from_millis(1100)).repeat().with_easing(ease_in_out),
                            move |d, t| d.top(px(-BAND + t * (h + BAND))),
                        )
                        .into_any_element(),
                ]
            }
            Scan::Landed(at) => {
                let Item::Local(path) = item else { return Vec::new() };
                let Some(r) = self.provenance.of(path) else { return Vec::new() };
                let c = purity_color(&r.wallpaper.purity);
                let tags: Vec<String> = r.tags.iter().flatten().take(3).map(|t| t.name.clone()).collect();
                let seed = format!("{key}-{}", at.elapsed().as_millis() / 100_000);
                let mut layers = edge_draw(c, w, h, &seed);
                // The tags it turned out to have, bursting up and away.
                layers.push(
                    h_flex()
                        .absolute()
                        .left(px(10.))
                        .right(px(10.))
                        .bottom(px(34.))
                        .justify_center()
                        .gap_1()
                        .children(tags.into_iter().map(move |t| {
                            div()
                                .px_1p5()
                                .rounded_full()
                                .bg(with_alpha(c, 0.85))
                                .text_size(px(10.))
                                .text_color(super::on_accent(c))
                                .truncate()
                                .child(t)
                        }))
                        .with_animation(SharedString::from(format!("burst-{seed}")), Animation::new(LANDED), |d, t| {
                            let a = if t < 0.12 {
                                t / 0.12
                            } else if t > 0.7 {
                                (1. - t) / 0.3
                            } else {
                                1.
                            };
                            d.opacity(a.clamp(0., 1.)).mb(px(16. * t))
                        })
                        .into_any_element(),
                );
                layers
            }
        }
    }

    // ---- the funnel ---------------------------------------------------------

    /// Under the Library bar while filtering: each filter as a stage, with
    /// how many wallpapers are left after it.
    pub(super) fn render_library_funnel(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let stages = self.stages();
        if stages.is_empty() {
            return None;
        }
        let p = pal();
        let total = self.library.len();
        let generation_n = self.lib_filter.generation;
        // Count after each stage, applied in order.
        let mut counts = Vec::with_capacity(stages.len());
        for i in 1..=stages.len() {
            counts.push(self.library.iter().filter(|path| self.passes(path, &stages[..i])).count());
        }
        let mut row = h_flex().gap_1p5().items_center().child(funnel_step(
            "funnel-all".into(),
            "All".into(),
            total,
            None,
            p,
            generation_n,
        ));
        for (i, (stage, n)) in stages.into_iter().zip(counts).enumerate() {
            let (label, tint) = match &stage {
                Stage::Purity(pu) => (purity_label(pu).to_owned(), Some(purity_color(pu))),
                Stage::Category(c) => (category_label(c).to_owned(), None),
                Stage::Tag(_, name) => (format!("#{name}"), None),
            };
            let remove = stage.clone();
            row = row.child(icon("chevron-right").size_3p5().text_color(p.dim)).child(
                funnel_step(SharedString::from(format!("funnel-{i}")), label, n, tint, p, generation_n)
                    .child(
                        div()
                            .id(("funnel-x", i))
                            .ml_0p5()
                            .rounded_full()
                            .text_color(p.dim)
                            .cursor_pointer()
                            .hover(move |s| s.text_color(p.fg))
                            .child(icon("close").size_3())
                            .on_click(cx.listener(move |this, _, _, cx| this.drop_stage(&remove, cx))),
                    )
                    // Each stage slides in from the left as it's added.
                    .with_animation(
                        SharedString::from(format!("funnel-in-{i}-{generation_n}")),
                        Animation::new(Duration::from_millis(260)).with_easing(ease_out_quint()),
                        |d, t| d.opacity(t).ml(px(-10. * (1. - t))),
                    ),
            );
        }
        Some(
            h_flex()
                .flex_none()
                .gap_2()
                .px(px(super::GRID_PAD))
                .pt_2()
                .child(div().flex_1().min_w_0().overflow_hidden().child(row))
                .child(
                    kit::Btn::new("funnel-clear")
                        .label("Clear")
                        .icon("rotate-ccw")
                        .ghost()
                        .sm()
                        .on_click(cx.listener(|this, _, _, cx| this.clear_lib_filter(cx))),
                )
                .into_any_element(),
        )
    }

    // ---- the sidebar --------------------------------------------------------

    /// The Library tab's sidebar: purity, category and tags, with counts.
    pub(super) fn render_library_side(&mut self, cx: &mut Context<Self>) -> Div {
        let p = pal();
        let mut purity_n: HashMap<String, usize> = HashMap::new();
        let mut category_n: HashMap<String, usize> = HashMap::new();
        // Tag id -> (name, its own purity, count), grouped by tag category.
        let mut groups: BTreeMap<String, HashMap<u64, (String, String, usize)>> = BTreeMap::new();
        let mut known = 0;
        for path in &self.library {
            let Some(r) = self.provenance.of(path) else { continue };
            known += 1;
            *purity_n.entry(r.wallpaper.purity.clone()).or_default() += 1;
            *category_n.entry(r.wallpaper.category.clone()).or_default() += 1;
            for t in r.tags.iter().flatten() {
                let cat = if t.category.is_empty() { "Other".to_owned() } else { t.category.clone() };
                groups
                    .entry(cat)
                    .or_default()
                    .entry(t.id)
                    .or_insert_with(|| (t.name.clone(), t.purity.clone(), 0))
                    .2 += 1;
            }
        }
        let unknown = self.library.len() - known;
        let active = self.lib_filter.active();
        let generation_n = self.lib_filter.generation;

        let purities: Vec<_> = PURITIES
            .iter()
            .enumerate()
            .filter(|(_, (k, _))| purity_n.contains_key(*k))
            .map(|(ix, &(k, label))| {
                let on = self.lib_filter.purity.contains(k);
                let n = purity_n[k];
                pop(
                    opt(("lib-purity", ix), format!("{label}  {n}"), on, Some(purity_color(k)), p).on_click(
                        cx.listener(move |this, _, _, cx| {
                            if !this.lib_filter.purity.remove(k) {
                                this.lib_filter.purity.insert(k.to_owned());
                            }
                            this.touch_filter(cx);
                        }),
                    ),
                    on,
                    format!("pp-{k}-{generation_n}"),
                )
            })
            .collect();
        let categories: Vec<_> = CATEGORIES
            .iter()
            .enumerate()
            .filter(|(_, (k, _))| category_n.contains_key(*k))
            .map(|(ix, &(k, label))| {
                let on = self.lib_filter.category.contains(k);
                let n = category_n[k];
                pop(
                    opt(("lib-cat", ix), format!("{label}  {n}"), on, None, p).on_click(cx.listener(
                        move |this, _, _, cx| {
                            if !this.lib_filter.category.remove(k) {
                                this.lib_filter.category.insert(k.to_owned());
                            }
                            this.touch_filter(cx);
                        },
                    )),
                    on,
                    format!("pc-{k}-{generation_n}"),
                )
            })
            .collect();

        // Tags, grouped by their category: biggest groups first, then by count.
        let query = self.lib_tag_query.read(cx).value().trim().to_lowercase();
        let hide = self.cfg.spoiler;
        let picked = self.lib_filter.tags.clone();
        let mut grouped: Vec<(String, Vec<(u64, String, String, usize)>)> = groups
            .into_iter()
            .map(|(cat, tags)| {
                let mut v: Vec<_> = tags
                    .into_iter()
                    .map(|(id, (name, purity, n))| (id, name, purity, n))
                    .filter(|(id, name, _, _)| {
                        picked.contains(id) || query.is_empty() || name.to_lowercase().contains(&query)
                    })
                    .collect();
                v.sort_by(|a, b| b.3.cmp(&a.3).then_with(|| a.1.cmp(&b.1)));
                (cat, v)
            })
            .filter(|(_, v)| !v.is_empty())
            .collect();
        grouped.sort_by_key(|(cat, v)| (std::cmp::Reverse(v.iter().map(|t| t.3).sum::<usize>()), cat.clone()));
        let tag_total: usize = grouped.iter().map(|(_, v)| v.len()).sum();
        let group_count = grouped.len();
        let groups_shown = if self.lib_filter.all_groups || !query.is_empty() { group_count } else { GROUPS_SHOWN };

        let mut tag_sections: Vec<AnyElement> = Vec::new();
        for (cat, tags) in grouped.into_iter().take(groups_shown) {
            let open = self.lib_filter.open_groups.contains(&cat) || !query.is_empty();
            let shown = if open { tags.len() } else { TAGS_PER_GROUP.min(tags.len()) };
            let more = tags.len() - shown;
            let chips = tags.into_iter().take(shown).map(|(id, name, purity, n)| {
                let on = picked.contains(&id);
                pop(
                    tag_chip(id, name, &purity, n, on, hide.hides(&purity), p)
                        .on_click(cx.listener(move |this, _, _, cx| this.toggle_tag(id, cx))),
                    on,
                    format!("tag-pop-{id}-{generation_n}"),
                )
            });
            let cat_key = cat.clone();
            tag_sections.push(
                v_flex()
                    .gap_1p5()
                    .child(
                        div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(p.dim).child(cat.to_uppercase()),
                    )
                    .child(h_flex().flex_wrap().gap_1p5().children(chips).when(more > 0, |d| {
                        d.child(
                            div()
                                .id(SharedString::from(format!("more-{cat_key}")))
                                .px_2()
                                .h(px(24.))
                                .flex()
                                .items_center()
                                .rounded_full()
                                .border_1()
                                .border_dashed()
                                .border_color(p.border_strong)
                                .text_xs()
                                .text_color(p.dim)
                                .cursor_pointer()
                                .hover(move |s| s.text_color(p.fg))
                                .child(format!("+{more}"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.lib_filter.open_groups.insert(cat_key.clone());
                                    cx.notify();
                                })),
                        )
                    }))
                    .into_any_element(),
            );
        }

        let header_clear = |h: Div| {
            h.when(active, |d| {
                d.child(
                    kit::Btn::new("lib-clear")
                        .label("Clear")
                        .ghost()
                        .sm()
                        .on_click(cx.listener(|this, _, _, cx| this.clear_lib_filter(cx))),
                )
            })
        };

        let col = v_flex().gap_3();
        if known == 0 {
            return col.child(side_card_with("folder", "Library", p, |h| h).child(
                div().text_xs().text_color(p.muted).child(if self.library.is_empty() {
                    "Wallpapers you download or set show up here."
                } else {
                    "Filters by purity, category and tags appear once your wallpapers are identified."
                }),
            ));
        }
        col.when(!purities.is_empty(), |d| {
            d.child(
                side_card_with("shield", "Purity", p, header_clear)
                    .child(opt_grid(purities.len() as u16).children(purities)),
            )
        })
        .when(!categories.is_empty(), |d| {
            d.child(
                side_card_with("layers", "Category", p, |h| h)
                    .child(opt_grid(categories.len() as u16).children(categories)),
            )
        })
        .child(
            side_card_with("tags", "Tags", p, |h| {
                h.child(div().text_xs().text_color(p.dim).child(tag_total.to_string()))
            })
            .gap_3()
            .child(
                div()
                    .h(px(30.))
                    .px_1()
                    .rounded_md()
                    .bg(p.bg)
                    .border_1()
                    .border_color(p.border_strong)
                    .child(Input::new(&self.lib_tag_query).appearance(false).small().cleanable(true)),
            )
            .children(tag_sections)
            .when(tag_total == 0, |d| {
                d.child(div().text_xs().text_color(p.dim).child(if query.is_empty() {
                    "Tags arrive as your wallpapers are identified."
                } else {
                    "No tags match."
                }))
            })
            .when(group_count > groups_shown, |d| {
                d.child(
                    kit::Btn::new("lib-all-groups")
                        .label(format!("Show all {group_count} categories"))
                        .ghost()
                        .sm()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.lib_filter.all_groups = true;
                            cx.notify();
                        })),
                )
            }),
        )
        .when(unknown > 0, |d| {
            d.child(div().px_1().text_xs().text_color(p.dim).child(format!(
                "{unknown} not identified yet{}",
                if active { ", so they're hidden while filtering" } else { "" }
            )))
        })
    }
}

/// A funnel stage: label and the count left after it. The count ticks up
/// or down to its new value whenever the filter changes.
fn funnel_step(
    id: SharedString,
    label: String,
    n: usize,
    tint: Option<Hsla>,
    p: Pal,
    generation_n: u64,
) -> Stateful<Div> {
    let c = tint.unwrap_or(p.accent);
    h_flex()
        .id(id.clone())
        .flex_none()
        .gap_1p5()
        .px_2p5()
        .h(px(26.))
        .rounded_full()
        .bg(with_alpha(c, 0.12))
        .border_1()
        .border_color(with_alpha(c, 0.45))
        .text_xs()
        .child(div().text_color(p.fg).child(label))
        .child(
            div().min_w(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(c).child(n.to_string()).with_animation(
                SharedString::from(format!("{id}-tick-{generation_n}-{n}")),
                Animation::new(Duration::from_millis(320)).with_easing(ease_out_quint()),
                |d, t| d.opacity(0.3 + 0.7 * t).mt(px(-6. * (1. - t))),
            ),
        )
}

/// A tag chip: its own purity as a dot and tinted edge, masked behind the
/// spoiler setting until hovered, like the cards it belongs to.
fn tag_chip(id: u64, name: String, purity: &str, n: usize, on: bool, masked: bool, p: Pal) -> Stateful<Div> {
    let tint = (purity != "sfw").then(|| purity_color(purity));
    let group = SharedString::from(format!("tag-{id}"));
    let label = if masked {
        div().relative().child(div().invisible().group_hover(group.clone(), |s| s.visible()).child(name)).child(
            div()
                .absolute()
                .inset_0()
                .rounded_sm()
                .bg(with_alpha(tint.unwrap_or(p.border_strong), 0.35))
                .group_hover(group.clone(), |s| s.invisible()),
        )
    } else {
        div().child(name)
    };
    h_flex()
        .id(("lib-tag", id as usize))
        .group(group)
        .gap_1p5()
        .px_2()
        .h(px(24.))
        .rounded_full()
        .border_1()
        .text_xs()
        .cursor_pointer()
        .map(|d| {
            if on {
                d.bg(with_alpha(tint.unwrap_or(p.accent), 0.22))
                    .border_color(with_alpha(tint.unwrap_or(p.accent), 0.8))
                    .text_color(p.fg)
            } else {
                d.bg(with_alpha(p.surface2, 0.6))
                    .border_color(tint.map_or(p.border, |c| with_alpha(c, 0.45)))
                    .text_color(p.muted)
                    .hover(move |s| s.border_color(tint.unwrap_or(p.border_strong)).text_color(p.fg))
            }
        })
        .when_some(tint, |d, c| d.child(div().flex_none().size(px(6.)).rounded_full().bg(c)))
        .child(label)
        .child(div().text_color(if on { p.muted } else { p.dim }).child(n.to_string()))
}

/// The purity edge drawing itself around a card, clockwise from the top
/// left, as four segments that each grow in turn.
fn edge_draw(c: Hsla, w: f32, h: f32, seed: &str) -> Vec<AnyElement> {
    const T: f32 = 2.;
    const R: f32 = 12.;
    let (sw, sh) = ((w - 2. * R).max(0.), (h - 2. * R).max(0.));
    let perimeter = 2. * (sw + sh);
    // Each segment: where it starts along the perimeter, and its length.
    let segs = [(0., sw), (sw, sh), (sw + sh, sw), (2. * sw + sh, sh)];
    segs.into_iter()
        .enumerate()
        .map(|(i, (start, len))| {
            let (a, b) = (start / perimeter, (start + len) / perimeter);
            div()
                .absolute()
                .rounded_full()
                .bg(c)
                .with_animation(
                    SharedString::from(format!("edge-{seed}-{i}")),
                    Animation::new(Duration::from_millis(700)).with_easing(ease_in_out),
                    move |d, t| {
                        let f = ((t - a) / (b - a)).clamp(0., 1.);
                        let fade = if t > 0.98 { 0.0 } else { 1.0 };
                        let d = d.opacity(fade);
                        match i {
                            // Top: left to right.
                            0 => d.top_0().left(px(R)).h(px(T)).w(px(sw * f)),
                            // Right: top to bottom.
                            1 => d.right_0().top(px(R)).w(px(T)).h(px(sh * f)),
                            // Bottom: right to left.
                            2 => d.bottom_0().right(px(R)).h(px(T)).w(px(sw * f)),
                            // Left: bottom to top.
                            _ => d.left_0().bottom(px(R)).w(px(T)).h(px(sh * f)),
                        }
                    },
                )
                .into_any_element()
        })
        .collect()
}

/// A chip that pops when it's picked.
fn pop(el: Stateful<Div>, on: bool, key: String) -> AnyElement {
    if on {
        el.with_animation(
            SharedString::from(key),
            Animation::new(Duration::from_millis(260)).with_easing(ease_out_quint()),
            |d, t| d.opacity(0.55 + 0.45 * t),
        )
        .into_any_element()
    } else {
        el.into_any_element()
    }
}
