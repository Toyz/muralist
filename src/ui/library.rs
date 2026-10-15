//! Filtering the Library by what its files are: purity, category and tags,
//! all from each file's provenance record. Purities and categories narrow
//! to any of those picked; tags narrow to files carrying all of them.

use std::collections::{BTreeSet, HashMap};

use gpui_kit::component::input::Input;
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{Item, Muralist, kit, opt, opt_grid, pal, purity_color, side_card_with, with_alpha};

/// Tags shown before "Show all".
const TAGS_SHOWN: usize = 24;

#[derive(Default)]
pub(super) struct LibFilter {
    pub purity: BTreeSet<String>,
    pub category: BTreeSet<String>,
    pub tags: BTreeSet<u64>,
    pub all_tags: bool,
}

impl LibFilter {
    pub fn active(&self) -> bool {
        !self.purity.is_empty() || !self.category.is_empty() || !self.tags.is_empty()
    }
}

const PURITIES: [(&str, &str); 3] = [("sfw", "SFW"), ("sketchy", "Sketchy"), ("nsfw", "NSFW")];
const CATEGORIES: [(&str, &str); 3] = [("general", "General"), ("anime", "Anime"), ("people", "People")];

impl Muralist {
    /// Whether a saved file passes the Library filter.
    fn lib_matches(&self, path: &std::path::Path) -> bool {
        let f = &self.lib_filter;
        if !f.active() {
            return true;
        }
        // Unidentified files can't be said to match anything.
        let Some(r) = self.provenance.of(path) else { return false };
        let w = &r.wallpaper;
        (f.purity.is_empty() || f.purity.contains(&w.purity))
            && (f.category.is_empty() || f.category.contains(&w.category))
            && f.tags.iter().all(|t| r.tags.as_ref().is_some_and(|tags| tags.iter().any(|x| x.id == *t)))
    }

    pub(super) fn library_items(&self) -> Vec<Item> {
        self.library.iter().filter(|p| self.lib_matches(p)).cloned().map(Item::Local).collect()
    }

    pub(super) fn clear_lib_filter(&mut self, cx: &mut Context<Self>) {
        let all_tags = self.lib_filter.all_tags;
        self.lib_filter = LibFilter { all_tags, ..Default::default() };
        cx.notify();
    }

    /// The Library tab's sidebar: purity, category and tags, with counts.
    pub(super) fn render_library_side(&mut self, cx: &mut Context<Self>) -> Div {
        let p = pal();
        let mut purity_n: HashMap<String, usize> = HashMap::new();
        let mut category_n: HashMap<String, usize> = HashMap::new();
        let mut tag_n: HashMap<u64, (String, usize)> = HashMap::new();
        let mut known = 0;
        for path in &self.library {
            let Some(r) = self.provenance.of(path) else { continue };
            known += 1;
            *purity_n.entry(r.wallpaper.purity.clone()).or_default() += 1;
            *category_n.entry(r.wallpaper.category.clone()).or_default() += 1;
            for t in r.tags.iter().flatten() {
                tag_n.entry(t.id).or_insert_with(|| (t.name.clone(), 0)).1 += 1;
            }
        }
        let unknown = self.library.len() - known;
        let active = self.lib_filter.active();

        let toggle = |set: &mut BTreeSet<String>, key: String| {
            if !set.remove(&key) {
                set.insert(key);
            }
        };
        let purities: Vec<_> = PURITIES
            .iter()
            .filter(|(k, _)| purity_n.contains_key(*k))
            .map(|&(k, label)| {
                let on = self.lib_filter.purity.contains(k);
                let n = purity_n[k];
                opt(("lib-purity", k.as_bytes()[0] as usize), format!("{label}  {n}"), on, Some(purity_color(k)), p)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        toggle(&mut this.lib_filter.purity, k.to_owned());
                        cx.notify();
                    }))
            })
            .collect();
        let categories: Vec<_> = CATEGORIES
            .iter()
            .filter(|(k, _)| category_n.contains_key(*k))
            .map(|&(k, label)| {
                let on = self.lib_filter.category.contains(k);
                let n = category_n[k];
                opt(("lib-cat", k.as_bytes()[0] as usize), format!("{label}  {n}"), on, None, p).on_click(cx.listener(
                    move |this, _, _, cx| {
                        toggle(&mut this.lib_filter.category, k.to_owned());
                        cx.notify();
                    },
                ))
            })
            .collect();

        // Tags: most common first, narrowed by the search field; picked ones
        // always stay visible.
        let query = self.lib_tag_query.read(cx).value().trim().to_lowercase();
        let mut tags: Vec<(u64, String, usize)> = tag_n.into_iter().map(|(id, (name, n))| (id, name, n)).collect();
        tags.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.1.cmp(&b.1)));
        let matching: Vec<_> = tags
            .iter()
            .filter(|(id, name, _)| {
                self.lib_filter.tags.contains(id) || query.is_empty() || name.to_lowercase().contains(&query)
            })
            .cloned()
            .collect();
        let total_tags = matching.len();
        let shown = if self.lib_filter.all_tags || !query.is_empty() { total_tags } else { TAGS_SHOWN.min(total_tags) };
        let chips = matching.into_iter().take(shown).map(|(id, name, n)| {
            let on = self.lib_filter.tags.contains(&id);
            h_flex()
                .id(("lib-tag", id as usize))
                .gap_1p5()
                .px_2()
                .h(px(24.))
                .rounded_full()
                .border_1()
                .text_xs()
                .cursor_pointer()
                .map(|d| {
                    if on {
                        d.bg(with_alpha(p.accent, 0.2)).border_color(with_alpha(p.accent, 0.7)).text_color(p.fg)
                    } else {
                        d.bg(with_alpha(p.surface2, 0.6))
                            .border_color(p.border)
                            .text_color(p.muted)
                            .hover(move |s| s.border_color(p.border_strong).text_color(p.fg))
                    }
                })
                .child(name)
                .child(div().text_color(if on { p.muted } else { p.dim }).child(n.to_string()))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !this.lib_filter.tags.remove(&id) {
                        this.lib_filter.tags.insert(id);
                    }
                    cx.notify();
                }))
        });

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

        let mut col = v_flex().gap_3();
        if known == 0 {
            return col.child(side_card_with("folder", "Library", p, |h| h).child(
                div().text_xs().text_color(p.muted).child(if self.library.is_empty() {
                    "Wallpapers you download or set show up here."
                } else {
                    "Filters by purity, category and tags appear once your wallpapers are identified."
                }),
            ));
        }
        if !purities.is_empty() {
            col = col.child(
                side_card_with("shield", "Purity", p, header_clear)
                    .child(opt_grid(purities.len() as u16).children(purities)),
            );
        }
        if !categories.is_empty() {
            col = col.child(
                side_card_with("layers", "Category", p, |h| h)
                    .child(opt_grid(categories.len() as u16).children(categories)),
            );
        }
        let more = (total_tags > shown).then(|| total_tags - shown);
        col.child(
            side_card_with("tags", "Tags", p, |h| {
                h.child(div().text_xs().text_color(p.dim).child(tags.len().to_string()))
            })
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
            .child(h_flex().flex_wrap().gap_1p5().children(chips))
            .when(tags.is_empty(), |d| {
                d.child(div().text_xs().text_color(p.dim).child("Tags arrive as your wallpapers are identified."))
            })
            .when_some(more, |d, more| {
                d.child(kit::Btn::new("lib-all-tags").label(format!("Show all ({more} more)")).ghost().sm().on_click(
                    cx.listener(|this, _, _, cx| {
                        this.lib_filter.all_tags = true;
                        cx.notify();
                    }),
                ))
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
