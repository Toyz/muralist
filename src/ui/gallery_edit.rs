//! Editing a gallery after the fact: its name, and for a smart gallery the
//! whole search behind it (text, purity, categories, sort, shape, how many
//! to keep), or for an imported collection which purities to bring in.
//! Saving applies it and refreshes the gallery, so nothing needs remaking.

use gpui_kit::component::input::Input;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Muralist, Pal, RATIO_PRESETS, RES_PRESETS, icon, kit, opt, opt_grid, pal, purity_color, segment, segmented_track,
    with_alpha,
};
use crate::gallery::{SavedSearch, Source};
use crate::wallhaven::{Purity, Sorting, TOP_RANGES};

pub(super) const LIMITS: [u32; 4] = [24, 48, 96, 192];
const CATEGORY_LABELS: [&str; 3] = ["General", "Anime", "People"];

/// Flips one of a set of switches, but never the last one left on: a
/// search with no purity or no category finds nothing.
pub(super) fn toggle_keeping_one(switches: &mut [bool; 3], i: usize) {
    switches[i] = !switches[i];
    if !switches.iter().any(|b| *b) {
        switches[i] = true;
    }
}

/// The gallery being edited, as it will be saved.
pub(super) struct GalleryDraft {
    pub id: u64,
    pub source: Source,
}

fn row(title: &str, hint: Option<&str>, control: impl IntoElement, p: Pal) -> Div {
    v_flex()
        .gap_2()
        .child(
            h_flex()
                .gap_2()
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(title.to_owned()))
                .children(hint.map(|h| div().text_xs().text_color(p.dim).child(h.to_owned()))),
        )
        .child(control)
}

fn field(input: impl IntoElement, p: Pal) -> Div {
    div()
        .h(px(34.))
        .px_1()
        .flex()
        .items_center()
        .rounded_lg()
        .bg(p.bg)
        .border_1()
        .border_color(p.border_strong)
        .child(div().flex_1().child(input))
}

impl Muralist {
    pub(super) fn open_gallery_editor(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(g) = self.galleries.get(id) else { return };
        let (name, source) = (g.name.clone(), g.source.clone());
        let query = match &source {
            Source::Smart { search, .. } => search.query.clone(),
            _ => String::new(),
        };
        self.edit_name.update(cx, |s, cx| s.set_value(name, window, cx));
        self.edit_query.update(cx, |s, cx| s.set_value(query, window, cx));
        self.gallery_edit = Some(GalleryDraft { id, source });
        self.focus.focus(window, cx);
        cx.notify();
    }

    fn close_gallery_editor(&mut self, cx: &mut Context<Self>) {
        self.gallery_edit = None;
        cx.notify();
    }

    fn save_gallery_editor(&mut self, cx: &mut Context<Self>) {
        let Some(draft) = self.gallery_edit.take() else { return };
        let name = self.edit_name.read(cx).value().trim().to_owned();
        let mut source = draft.source;
        if let Source::Smart { search, .. } = &mut source {
            let query = self.edit_query.read(cx).value().trim().to_owned();
            // A new query gets a new label, worked out on refresh.
            if query != search.query {
                search.query = query;
                search.label = None;
            }
        }
        let Some(g) = self.galleries.get_mut(draft.id) else { return };
        let changed = g.source != source;
        if !name.is_empty() {
            g.name = name;
        }
        g.source = source;
        if changed {
            g.refreshed = 0;
        }
        if let Err(e) = self.galleries.save() {
            self.set_status(format!("Saving galleries failed: {e:#}"), true, cx);
        }
        if changed {
            self.refresh_gallery(draft.id, cx);
        }
        cx.notify();
    }

    fn edit_search(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut SavedSearch, &mut u32)) {
        if let Some(GalleryDraft { source: Source::Smart { search, limit }, .. }) = &mut self.gallery_edit {
            f(search, limit);
        }
        cx.notify();
    }

    pub(super) fn render_gallery_editor(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let draft = self.gallery_edit.as_ref()?;
        let p = pal();
        let has_key = self.client.has_key();
        let source = draft.source.clone();

        let mut body =
            v_flex().gap_5().child(row("Name", None, field(Input::new(&self.edit_name).appearance(false), p), p));

        match source {
            Source::Smart { search, limit } => {
                let purity = search.purity;
                let purity_opts = (0..3).filter(|i| *i != 2 || has_key).map(|i| {
                    opt(("ge-purity", i), Purity::ALL[i].label(), purity[i], Some(purity_color(Purity::ALL[i])), p)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.edit_search(cx, |s, _| {
                                toggle_keeping_one(&mut s.purity, i);
                            });
                        }))
                });
                let categories = search.categories;
                let category_opts = (0..3).map(|i| {
                    opt(("ge-cat", i), CATEGORY_LABELS[i], categories[i], None, p).on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.edit_search(cx, |s, _| {
                                toggle_keeping_one(&mut s.categories, i);
                            });
                        },
                    ))
                });
                let sorting = Sorting::from_api(&search.sorting).unwrap_or(Sorting::DateAdded);
                let sort_opts = Sorting::ALL.iter().enumerate().map(|(i, &s)| {
                    opt(("ge-sort", i), s.label(), sorting == s, None, p).flex_grow(1.).px_2p5().on_click(
                        cx.listener(move |this, _, _, cx| this.edit_search(cx, |q, _| q.sorting = s.api().to_owned())),
                    )
                });
                let range = search.top_range.clone();
                let range_segs = TOP_RANGES.iter().enumerate().map(|(i, &r)| {
                    segment(("ge-range", i), r, range == r, p).px_0().on_click(
                        cx.listener(move |this, _, _, cx| this.edit_search(cx, |q, _| q.top_range = r.into())),
                    )
                });
                let ratios = search.ratios.clone();
                let ratio_opts = RATIO_PRESETS.iter().enumerate().map(|(i, &(label, value))| {
                    opt(("ge-ratio", i), label, ratios.as_deref() == value, None, p).on_click(cx.listener(
                        move |this, _, _, cx| this.edit_search(cx, |q, _| q.ratios = value.map(str::to_owned)),
                    ))
                });
                let atleast = search.atleast.clone();
                let res_opts = RES_PRESETS.iter().enumerate().map(|(i, &(label, value))| {
                    opt(("ge-res", i), label, atleast.as_deref() == value && search.resolutions.is_none(), None, p)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.edit_search(cx, |q, _| {
                                q.atleast = value.map(str::to_owned);
                                q.resolutions = None;
                            });
                        }))
                });
                let limit_segs = LIMITS.iter().enumerate().map(|(i, &n)| {
                    segment(("ge-limit", i), format!("{n}"), limit == n, p)
                        .on_click(cx.listener(move |this, _, _, cx| this.edit_search(cx, |_, l| *l = n)))
                });
                body = body
                    .child(row(
                        "Search",
                        Some("Words, or id:123, @user, like:abc123"),
                        field(Input::new(&self.edit_query).appearance(false), p),
                        p,
                    ))
                    .child(row(
                        "Purity",
                        (!has_key).then_some("NSFW needs an API key"),
                        opt_grid(if has_key { 3 } else { 2 }).children(purity_opts),
                        p,
                    ))
                    .child(row("Categories", None, opt_grid(3).children(category_opts), p))
                    .child(row("Sort", None, h_flex().flex_wrap().gap_1p5().children(sort_opts), p))
                    .when(sorting == Sorting::Toplist, |d| {
                        d.child(row("Top of", None, segmented_track(p).children(range_segs), p))
                    })
                    .child(row("Aspect ratio", None, opt_grid(RATIO_PRESETS.len() as u16).children(ratio_opts), p))
                    .child(row("At least", None, opt_grid(RES_PRESETS.len() as u16).children(res_opts), p))
                    .child(row(
                        "Keep",
                        Some("the top results, refreshed daily"),
                        segmented_track(p).children(limit_segs),
                        p,
                    ));
            }
            Source::Wallhaven { purity, .. } => {
                let current = purity.unwrap_or(self.purity);
                let purity_opts = (0..3).filter(|i| *i != 2 || has_key).map(|i| {
                    opt(("ge-wpurity", i), Purity::ALL[i].label(), current[i], Some(purity_color(Purity::ALL[i])), p)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(GalleryDraft { source: Source::Wallhaven { purity, .. }, .. }) =
                                &mut this.gallery_edit
                            {
                                let mut next = purity.unwrap_or(current);
                                toggle_keeping_one(&mut next, i);
                                *purity = Some(next);
                            }
                            cx.notify();
                        }))
                });
                body = body.child(row(
                    "Purity",
                    Some(if has_key { "which wallpapers to import" } else { "NSFW needs an API key" }),
                    opt_grid(if has_key { 3 } else { 2 }).children(purity_opts),
                    p,
                ));
            }
            Source::Manual => {}
        }

        let card = v_flex()
            .id("gallery-editor")
            .w(px(560.))
            .max_h(relative(0.9))
            .rounded_xl()
            .overflow_hidden()
            .bg(p.surface)
            .border_1()
            .border_color(p.border_strong)
            .shadow_lg()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                h_flex()
                    .flex_none()
                    .h(px(56.))
                    .px_5()
                    .gap_2p5()
                    .border_b_1()
                    .border_color(p.border)
                    .child(icon("sliders-horizontal").size_4().text_color(p.accent))
                    .child(div().flex_1().font_weight(FontWeight::SEMIBOLD).child("Edit gallery"))
                    .child(
                        kit::Btn::new("ge-close")
                            .icon("close")
                            .ghost()
                            .sm()
                            .on_click(cx.listener(|this, _, _, cx| this.close_gallery_editor(cx))),
                    ),
            )
            .child(div().id("ge-body").flex_1().min_h_0().overflow_y_scroll().child(body.p_5()))
            .child(
                h_flex()
                    .flex_none()
                    .justify_end()
                    .gap_2()
                    .px_5()
                    .h(px(60.))
                    .border_t_1()
                    .border_color(p.border)
                    .bg(with_alpha(p.surface2, 0.5))
                    .child(
                        kit::Btn::new("ge-cancel")
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, _, cx| this.close_gallery_editor(cx))),
                    )
                    .child(
                        kit::Btn::new("ge-save")
                            .label("Save and refresh")
                            .icon("check")
                            .primary()
                            .on_click(cx.listener(|this, _, _, cx| this.save_gallery_editor(cx))),
                    ),
            )
            .with_animation(
                "gallery-editor-open",
                Animation::new(std::time::Duration::from_millis(180)).with_easing(ease_out_quint()),
                |d, t| d.opacity(t).mt(px(12. * (1. - t))),
            );

        Some(
            div()
                .id("gallery-editor-overlay")
                .track_focus(&self.focus)
                .occlude()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(hsla(0., 0., 0., 0.6))
                .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                    if ev.keystroke.key == "escape" {
                        this.close_gallery_editor(cx);
                        cx.stop_propagation();
                    }
                }))
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.close_gallery_editor(cx)))
                .child(card)
                .into_any_element(),
        )
    }
}
