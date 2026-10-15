//! The search box: tinted with the results' colors, a drop target that
//! searches by an image's color, "understood as" chips for smart queries,
//! and recent searches with a peek at their results.

use std::path::PathBuf;

use gpui_kit::component::input::Input;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::slot::lab;
use super::smartq::{self, ChipKind};
use super::{
    GRID_PAD, Item, Muralist, TITLE_H, WALLHAVEN_COLORS, adaptive, drag, hex, icon, kit, pal, parse_hex, radius,
    thousands, with_alpha,
};
use crate::thumbs::Job;

/// The Wallhaven palette color nearest to an image's character: its
/// dominant color, unless that's a dull gray/black and a vivid one exists.
fn pick_palette_color(colors: &[Hsla]) -> Option<&'static str> {
    let first = *colors.first()?;
    let target = if first.s < 0.12 || first.l < 0.08 {
        colors.iter().take(4).copied().find(|c| c.s >= 0.3 && c.l > 0.15).unwrap_or(first)
    } else {
        first
    };
    let t = lab(target);
    WALLHAVEN_COLORS.iter().copied().min_by(|a, b| {
        let da = lab(hex(u32::from_str_radix(a, 16).unwrap_or(0)));
        let db = lab(hex(u32::from_str_radix(b, 16).unwrap_or(0)));
        let d = |x: [f32; 3]| (x[0] - t[0]).powi(2) + (x[1] - t[1]).powi(2) + (x[2] - t[2]).powi(2);
        d(da).total_cmp(&d(db))
    })
}

impl Muralist {
    /// Search for wallpapers in the color of an image file.
    pub(super) fn search_by_image(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.set_status(format!("Reading colors from {name}..."), false, cx);
        cx.spawn(async move |this, cx| {
            let colors = cx.background_executor().spawn(async move { adaptive::image_palette(&path) }).await;
            this.update(cx, |this, cx| this.search_by_colors(&colors, &name, cx)).ok();
        })
        .detach();
    }

    pub(super) fn search_by_colors(&mut self, colors: &[Hsla], source: &str, cx: &mut Context<Self>) {
        let Some(hex) = pick_palette_color(colors) else {
            self.set_status(format!("Couldn't read any colors from {source}"), true, cx);
            return;
        };
        self.color = Some(hex.to_owned());
        self.search(true, cx);
        self.set_status(format!("Searching wallpapers in #{hex}, from {source}"), false, cx);
    }

    pub(super) fn render_searchbox(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        let p = pal();
        // Tint: the dominant colors of the first few results, faintly.
        let tints: Vec<Hsla> =
            self.results.iter().take(8).filter_map(|w| w.colors.first().and_then(|c| parse_hex(c))).collect();

        let count = (self.total > 0).then(|| thousands(self.total));
        let suffix = h_flex().gap_2().children(count.map(|c| div().text_xs().text_color(p.dim).child(c))).child(
            if self.searching {
                Spinner::new().xsmall().color(p.accent).into_any_element()
            } else {
                super::keycap("Enter", p).into_any_element()
            },
        );

        let scope_pill = self.scope.clone().map(|scope| {
            let (lead, name, detail) = self.scope_label(&scope, cx);
            kit::surface(h_flex(), p)
                .id("scope-pill")
                .flex_none()
                .gap_1p5()
                .pl_2()
                .pr_0p5()
                .h(px(24.))
                .rounded_full()
                .overflow_hidden()
                .text_xs()
                .child(lead)
                .child(div().text_color(p.fg).child(name))
                .children(detail.map(|d| div().text_color(p.dim).child(d)))
                .child(
                    div()
                        .id("scope-x")
                        .size(px(18.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .text_color(p.dim)
                        .cursor_pointer()
                        .hover(move |s| s.bg(p.elevated).text_color(p.fg))
                        .child(icon("close").size_3())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.scope = None;
                            this.search(true, cx);
                        })),
                )
        });
        let prefix = h_flex().gap_2().child(icon("search").size_4().text_color(p.muted)).children(scope_pill);

        div()
            .id("searchbox")
            .flex_1()
            .min_w_0()
            .h(px(38.))
            .flex()
            .items_center()
            .px_1()
            .rounded_lg()
            .border_1()
            .border_color(if self.search_focused { with_alpha(p.accent, 0.7) } else { p.border_strong })
            .bg(super::field_wash(&tints, p))
            .drag_over::<ExternalPaths>(move |s, _, _, _| s.border_color(p.accent).bg(with_alpha(p.accent, 0.18)))
            .drag_over::<drag::CardDrag>(move |s, _, _, _| s.border_color(p.accent).bg(with_alpha(p.accent, 0.18)))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                if let Some(path) = paths.paths().first() {
                    this.search_by_image(path.clone(), cx);
                }
            }))
            .on_drop(cx.listener(|this, drag: &drag::CardDrag, _, cx| {
                let colors = super::gridmodes::item_colors(&drag.item, &this.local_palettes);
                if !colors.is_empty() {
                    let name = match &drag.item {
                        Item::Remote(w) => w.id.clone(),
                        Item::Local(p) => super::file_name(p),
                    };
                    this.search_by_colors(&colors, &name, cx);
                } else if let Some(t) = &drag.thumb {
                    this.search_by_image(t.clone(), cx);
                }
            }))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(&self.query).appearance(false).prefix(prefix).suffix(suffix).cleanable(true)),
            )
    }

    /// Readable label for a scope: (lead element, name, detail).
    pub(super) fn scope_label(
        &mut self,
        scope: &smartq::Scope,
        cx: &mut Context<Self>,
    ) -> (AnyElement, String, Option<String>) {
        let p = pal();
        match scope {
            smartq::Scope::Tag(id) => match self.tag_label(*id, cx) {
                // The tag's purity leads: a dot in its color, then its name.
                Some(t) => {
                    let c = super::purity_color(&t.purity);
                    let lead = h_flex()
                        .gap_1()
                        .child(icon("book-open").size_3().text_color(c))
                        .child(
                            h_flex()
                                .gap_1()
                                .px_1p5()
                                .rounded_full()
                                .bg(with_alpha(c, 0.18))
                                .text_size(px(9.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(c)
                                .child(div().size(px(5.)).rounded_full().bg(c))
                                .child(super::purity_label(&t.purity)),
                        )
                        .into_any_element();
                    (lead, t.name, (!t.category.is_empty()).then_some(t.category))
                }
                None => (
                    icon("book-open").size_3().text_color(p.accent).into_any_element(),
                    "Tag".into(),
                    Some("loading".into()),
                ),
            },
            smartq::Scope::Like(id) => {
                let url = smartq::small_thumb_url(id);
                let thumb = self.thumb(format!("wh-{id}"), |key| Job::Remote { key, url });
                let lead = div()
                    .w(px(22.))
                    .h(px(14.))
                    .rounded_sm()
                    .overflow_hidden()
                    .bg(p.elevated)
                    .when_some(thumb, |d, t| d.child(radius(img(t).size_full().object_fit(ObjectFit::Cover), 2.)))
                    .into_any_element();
                (lead, "Similar to".into(), Some(id.clone()))
            }
            smartq::Scope::User(name) => (
                icon("user").size_3().text_color(p.accent).into_any_element(),
                format!("@{name}"),
                Some("uploads".into()),
            ),
        }
    }

    /// A tag id's name and category, fetched once and cached.
    pub(super) fn tag_label(&mut self, id: u64, cx: &mut Context<Self>) -> Option<crate::wallhaven::Tag> {
        if let Some(known) = self.tag_names.get(&id) {
            return known.clone();
        }
        // A failed lookup is tried again, but not every frame.
        if self.tag_failed.get(&id).is_some_and(|at| at.elapsed() < std::time::Duration::from_secs(30)) {
            return None;
        }
        self.tag_names.insert(id, None);
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { client.tag(id) }).await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(tag) => {
                        this.tag_names.insert(id, Some(tag));
                    }
                    Err(_) => {
                        this.tag_names.remove(&id);
                        this.tag_failed.insert(id, std::time::Instant::now());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        None
    }

    /// "Understood as" chips for filters the smart query applied, plus
    /// dashed suggestions for ambiguous words (click to apply).
    pub(super) fn render_query_chips(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = pal();
        // Parsed again only when the text changes, not every frame.
        let raw = self.query.read(cx).value().to_string();
        let parsed = match &self.query_parsed {
            Some((text, parsed)) if *text == raw => parsed.clone(),
            _ => {
                let parsed = std::rc::Rc::new(smartq::parse(&raw));
                self.query_parsed = Some((raw.clone(), parsed.clone()));
                parsed
            }
        };
        if parsed.chips.is_empty() && parsed.suggestions.is_empty() {
            return None;
        }

        let mut chips: Vec<AnyElement> = Vec::new();
        for (i, chip) in parsed.chips.iter().cloned().enumerate() {
            let words = chip.words.clone();
            let raw = raw.clone();
            let token = chip.explicit.clone();
            let lower = token.to_lowercase();

            // Operators resolve into something readable.
            let mut label = chip.label.clone();
            let mut sub: Option<String> = None;
            let mut lead: AnyElement = icon("asterisk").size_3().text_color(p.muted).into_any_element();
            if let Some(id) = lower.strip_prefix("id:").and_then(|v| v.parse::<u64>().ok()) {
                let tag = self.tag_label(id, cx);
                let tint = tag.as_ref().map_or(p.accent, |t| super::purity_color(&t.purity));
                match tag {
                    Some(t) => {
                        label = t.name;
                        sub = (!t.category.is_empty()).then_some(t.category);
                    }
                    None => label = format!("tag {id}"),
                }
                lead = icon("book-open").size_3().text_color(tint).into_any_element();
            } else if let Some(id) = token.strip_prefix("like:").filter(|v| smartq::is_wallpaper_id(v)) {
                let url = smartq::small_thumb_url(id);
                let thumb = self.thumb(format!("wh-{id}"), |key| Job::Remote { key, url });
                label = format!("like {id}");
                lead = div()
                    .w(px(22.))
                    .h(px(14.))
                    .rounded_sm()
                    .overflow_hidden()
                    .bg(p.elevated)
                    .when_some(thumb, |d, t| d.child(radius(img(t).size_full().object_fit(ObjectFit::Cover), 2.)))
                    .into_any_element();
            } else if token.starts_with('@') {
                lead = icon("user").size_3().text_color(p.muted).into_any_element();
            } else if token.starts_with('-') {
                lead = icon("ban").size_3().text_color(p.error).into_any_element();
            } else if token.starts_with('+') {
                lead = icon("plus").size_3().text_color(p.ok).into_any_element();
            }

            let (name, tint) = match chip.kind {
                ChipKind::Color => ("palette", None),
                ChipKind::Ratio => ("frame", None),
                ChipKind::Resolution => ("maximize", None),
                ChipKind::Purity => ("eye", Some(super::purity_color(&chip.label))),
                ChipKind::Category => ("folder", None),
                ChipKind::Sort => ("sort-descending", None),
                ChipKind::Operator => ("asterisk", None),
            };
            let lead = match (&chip.swatch, chip.kind) {
                (Some(hx), _) => div()
                    .size(px(12.))
                    .rounded_full()
                    .border_1()
                    .border_color(p.border_strong)
                    .bg(parse_hex(hx).unwrap_or(p.muted))
                    .into_any_element(),
                (None, ChipKind::Operator) => lead,
                (None, _) => icon(name).size_3().text_color(tint.unwrap_or(p.muted)).into_any_element(),
            };

            chips.push(
                kit::surface(h_flex(), p)
                    .id(("qchip", i))
                    .gap_1p5()
                    .pl_2()
                    .pr_1()
                    .h(px(26.))
                    .rounded_full()
                    .overflow_hidden()
                    .text_xs()
                    .text_color(tint.unwrap_or(p.fg))
                    .child(lead)
                    .child(label)
                    .children(sub.map(|s| div().text_color(p.dim).child(s)))
                    .child(
                        div()
                            .id(("qchip-x", i))
                            .size(px(18.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .text_color(p.dim)
                            .cursor_pointer()
                            .hover(move |s| s.bg(p.elevated).text_color(p.fg))
                            .child(icon("close").size_3())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let rest = smartq::without(&raw, &words);
                                this.query.update(cx, |s, cx| s.set_value(rest, window, cx));
                                this.search(true, cx);
                            })),
                    )
                    .into_any_element(),
            );
        }

        let suggestions: Vec<AnyElement> = parsed
            .suggestions
            .iter()
            .cloned()
            .enumerate()
            .map(|(i, sug)| {
                let words = sug.words.clone();
                let explicit = sug.explicit.clone();
                let raw = raw.clone();
                let text = match sug.kind {
                    ChipKind::Color => format!("{} as color", sug.label),
                    ChipKind::Category => format!("{} category", sug.label),
                    ChipKind::Sort => format!("sort by {}", sug.label),
                    ChipKind::Ratio => format!("{} only", sug.label),
                    _ => sug.label.clone(),
                };
                h_flex()
                    .id(("qsug", i))
                    .gap_1p5()
                    .px_2()
                    .h(px(26.))
                    .rounded_full()
                    .border_1()
                    .border_dashed()
                    .border_color(p.border_strong)
                    .text_xs()
                    .text_color(p.muted)
                    .cursor_pointer()
                    .hover(move |s| s.border_color(p.accent).text_color(p.fg))
                    .when_some(sug.swatch, |d, hx| {
                        d.child(div().size(px(10.)).rounded_full().bg(parse_hex(&hx).unwrap_or(p.muted)))
                    })
                    .child(icon("plus").size_3())
                    .child(text)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let next = smartq::promote(&raw, &words, &explicit);
                        this.query.update(cx, |s, cx| s.set_value(next, window, cx));
                        this.search(true, cx);
                    }))
                    .into_any_element()
            })
            .collect();

        let has_chips = !chips.is_empty();
        let has_suggestions = !suggestions.is_empty();
        Some(
            h_flex()
                .flex_wrap()
                .gap_1p5()
                .px(px(GRID_PAD))
                .pt_2()
                .when(has_chips, |d| d.child(div().text_xs().text_color(p.dim).mr_1().child("Understood as")))
                .children(chips)
                .when(has_suggestions, |d| {
                    d.child(div().text_xs().text_color(p.dim).mx_1().child(if has_chips {
                        "also"
                    } else {
                        "Use as filter?"
                    }))
                })
                .children(suggestions)
                .into_any_element(),
        )
    }

    /// Recent searches, when the empty search box is focused.
    pub(super) fn render_recent(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.search_focused || !self.query.read(cx).value().is_empty() || self.cfg.recent.is_empty() {
            return None;
        }
        let p = pal();
        let recent = self.cfg.recent.clone();
        let rows: Vec<AnyElement> = recent
            .into_iter()
            .enumerate()
            .map(|(i, r)| {
                let thumbs: Vec<AnyElement> = r
                    .thumbs
                    .iter()
                    .map(|(id, url)| {
                        let url = url.clone();
                        let t = self.thumb(format!("wh-{id}"), |key| Job::Remote { key, url });
                        div()
                            .w(px(46.))
                            .h(px(30.))
                            .rounded_md()
                            .overflow_hidden()
                            .bg(p.surface2)
                            .when_some(t, |d, t| d.child(radius(img(t).size_full().object_fit(ObjectFit::Cover), 5.)))
                            .into_any_element()
                    })
                    .collect();
                let text = r.text.clone();
                let label = match smartq::Scope::parse(&r.text) {
                    Some(scope) => {
                        let (_, name, detail) = self.scope_label(&scope, cx);
                        match detail {
                            Some(d) => format!("{name} {d}"),
                            None => name,
                        }
                    }
                    None => r.text.clone(),
                };
                h_flex()
                    .id(("recent", i))
                    .gap_3()
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .cursor_pointer()
                    .hover(move |s| s.bg(p.elevated))
                    .child(icon("rotate-cw").size_3p5().text_color(p.dim))
                    .child(div().flex_1().min_w_0().truncate().text_sm().child(label))
                    .child(h_flex().gap_1().children(thumbs))
                    // Mouse-down, so it lands before the box loses focus.
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.search_for(text.clone(), window, cx);
                        }),
                    )
                    .into_any_element()
            })
            .collect();
        Some(
            v_flex()
                .id("recent-searches")
                .occlude()
                .absolute()
                .top(px(TITLE_H - 6.))
                .left(px(GRID_PAD))
                .right(px(GRID_PAD + 140.))
                .p_1p5()
                .rounded_xl()
                .bg(p.surface)
                .border_1()
                .border_color(p.border_strong)
                .shadow_lg()
                .child(
                    h_flex()
                        .justify_between()
                        .px_3()
                        .pt_1()
                        .pb_1p5()
                        .child(div().text_xs().text_color(p.dim).child("RECENT"))
                        .child(kit::Btn::new("recent-clear").label("Clear").ghost().sm().render().on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                cx.stop_propagation();
                                this.cfg.recent.clear();
                                if let Err(e) = this.cfg.save() {
                                    eprintln!("saving config: {e:#}");
                                }
                                cx.notify();
                            }),
                        )),
                )
                .children(rows)
                .into_any_element(),
        )
    }
}
