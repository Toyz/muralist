//! Right-click a gallery in the sidebar and it lifts out into a popout
//! beside it: its cover on glass in its own colors, its top-level filters
//! as quick toggles, and its actions. Filter changes refresh the gallery
//! when the popout closes; it settles back into its tile as it does.

use std::time::Duration;

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::galleries::item_of;
use super::{Muralist, box_ratio, glass, gridmodes, icon, pal, purity_color, radius, with_alpha};
use crate::config::RotateSource;
use crate::gallery::{Entry, Source};
use crate::wallhaven::Sorting;

/// The grown card: its width, and its height by what it holds.
const POP_W: f32 = 380.;
const POP_H_SMART: f32 = 470.;
const POP_H_COLLECTION: f32 = 300.;
const POP_H_MANUAL: f32 = 230.;
const R: f32 = 14.;
const OPEN_MS: u64 = 200;
const CLOSE_MS: u64 = 170;
const LIMITS: [u32; 4] = [24, 48, 96, 192];

pub(super) struct GalleryPop {
    pub id: u64,
    /// The tile it lifted from, in window coordinates.
    pub origin: Bounds<Pixels>,
    /// The gallery's source when it opened, to tell whether it changed.
    pub opened_with: Source,
    pub seq: u64,
    pub closing: bool,
}

impl Muralist {
    pub(super) fn open_gallery_pop(&mut self, id: u64, origin: Bounds<Pixels>, cx: &mut Context<Self>) {
        let Some(g) = self.galleries.get(id) else { return };
        self.context = None;
        self.gallery_pop_seq += 1;
        self.gallery_pop =
            Some(GalleryPop { id, origin, opened_with: g.source.clone(), seq: self.gallery_pop_seq, closing: false });
        cx.notify();
    }

    /// Settles back into the tile; refreshes the gallery if its filters changed.
    pub(super) fn close_gallery_pop(&mut self, cx: &mut Context<Self>) {
        let Some(pop) = self.gallery_pop.as_mut().filter(|p| !p.closing) else { return };
        pop.closing = true;
        let (id, seq, opened_with) = (pop.id, pop.seq, pop.opened_with.clone());
        let changed = self.galleries.get(id).is_some_and(|g| g.source != opened_with);
        if changed && let Err(e) = self.galleries.save() {
            self.set_status(format!("Saving galleries failed: {e:#}"), true, cx);
        }
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(CLOSE_MS)).await;
            this.update(cx, |this, cx| {
                if this.gallery_pop.as_ref().is_some_and(|p| p.seq == seq) {
                    this.gallery_pop = None;
                }
                // Only now, with the card back in its tile, so nothing
                // moves under the animation.
                if changed {
                    this.refresh_gallery(id, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn pop_source(&mut self, f: impl FnOnce(&mut Source)) {
        let Some(id) = self.gallery_pop.as_ref().map(|p| p.id) else { return };
        if let Some(g) = self.galleries.get_mut(id) {
            f(&mut g.source);
        }
    }

    pub(super) fn render_gallery_pop(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let pop = self.gallery_pop.as_ref()?;
        let (id, origin, seq, closing) = (pop.id, pop.origin, pop.seq, pop.closing);
        let g = self.galleries.get(id)?.clone();
        let p = pal();
        let has_key = self.client.has_key();

        let first: Option<Entry> = g.items.first().cloned();
        let colors: Vec<Hsla> =
            g.items.iter().take(3).flat_map(|e| gridmodes::item_colors(&item_of(e), &self.local_palettes)).collect();
        let c = glass::glow_color(&colors).unwrap_or(p.accent);
        let cover = first.and_then(|e| match e {
            Entry::Wallhaven(w) => self.remote_thumb(&w, true).or_else(|| self.remote_thumb(&w, false)),
            Entry::Local(path) => self.local_thumb(&path),
        });

        // Lift from the tile to beside the sidebar, level with it.
        let (vw, vh) = self.viewport;
        let (fx, fy, fw, fh) = (
            f32::from(origin.origin.x),
            f32::from(origin.origin.y),
            f32::from(origin.size.width),
            f32::from(origin.size.height),
        );
        // Grow around the tile's own center, like the cards do, kept in the window.
        let pop_h = match g.source {
            Source::Smart { .. } => POP_H_SMART,
            Source::Wallhaven { .. } => POP_H_COLLECTION,
            Source::Manual => POP_H_MANUAL,
        };
        let tx = (fx + fw / 2. - POP_W / 2.).clamp(12., (vw - POP_W - 12.).max(12.));
        let ty = (fy + fh / 2. - pop_h / 2.).clamp(12., (vh - pop_h - 12.).max(12.));

        // ---- quick filters ----
        let chip = |id: ElementId, label: String, on: bool, tint: Option<Hsla>| {
            let t = tint.unwrap_or(c);
            h_flex()
                .id(id)
                .flex_1()
                .justify_center()
                .gap_1()
                .h(px(28.))
                .rounded_md()
                .border_1()
                .text_xs()
                .cursor_pointer()
                .map(|d| {
                    if on {
                        d.bg(with_alpha(t, 0.3)).border_color(with_alpha(t, 0.9)).text_color(white())
                    } else {
                        d.bg(hsla(0., 0., 1., 0.05))
                            .border_color(hsla(0., 0., 1., 0.1))
                            .text_color(hsla(0., 0., 1., 0.7))
                            .hover(move |s| s.border_color(with_alpha(t, 0.6)).text_color(white()))
                    }
                })
                .when_some(tint, |d, t| d.child(div().size(px(6.)).rounded_full().bg(t)))
                .child(label)
        };
        let label = |text: &str| div().text_xs().text_color(hsla(0., 0., 1., 0.55)).child(text.to_uppercase());

        let purity_of = |s: &Source, fallback: [bool; 3]| match s {
            Source::Smart { search, .. } => Some(search.purity),
            Source::Wallhaven { purity, .. } => Some(purity.unwrap_or(fallback)),
            Source::Manual => None,
        };
        let mut filters = v_flex().gap_2p5();
        if let Some(purity) = purity_of(&g.source, self.purity) {
            let fallback = self.purity;
            let row = (0..3).filter(|i| *i != 2 || has_key).map(|i| {
                let key = ["sfw", "sketchy", "nsfw"][i];
                chip(("pop-purity", i).into(), ["SFW", "Sketchy", "NSFW"][i].into(), purity[i], Some(purity_color(key)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.pop_source(|s| {
                            let flip = |mut v: [bool; 3]| {
                                v[i] = !v[i];
                                if !v.iter().any(|b| *b) {
                                    v[i] = true;
                                }
                                v
                            };
                            match s {
                                Source::Smart { search, .. } => search.purity = flip(search.purity),
                                Source::Wallhaven { purity, .. } => *purity = Some(flip(purity.unwrap_or(fallback))),
                                Source::Manual => {}
                            }
                        });
                        cx.notify();
                    }))
            });
            filters = filters.child(v_flex().gap_1p5().child(label("Purity")).child(h_flex().gap_1p5().children(row)));
        }
        if let Source::Smart { search, limit } = &g.source {
            let cats = (0..3).map(|i| {
                chip(("pop-cat", i).into(), ["General", "Anime", "People"][i].into(), search.categories[i], None)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.pop_source(|s| {
                            if let Source::Smart { search, .. } = s {
                                search.categories[i] = !search.categories[i];
                                if !search.categories.iter().any(|b| *b) {
                                    search.categories[i] = true;
                                }
                            }
                        });
                        cx.notify();
                    }))
            });
            let sorting = Sorting::from_api(&search.sorting).unwrap_or(Sorting::DateAdded);
            let sorts = [Sorting::DateAdded, Sorting::Hot, Sorting::Toplist, Sorting::Random].into_iter().map(|s| {
                chip(("pop-sort", s as usize).into(), s.label().into(), sorting == s, None).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.pop_source(|src| {
                            if let Source::Smart { search, .. } = src {
                                search.sorting = s.api().to_owned();
                            }
                        });
                        cx.notify();
                    },
                ))
            });
            let current = *limit;
            let limits = LIMITS.iter().map(|&n| {
                chip(("pop-limit", n as usize).into(), n.to_string(), current == n, None).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.pop_source(|s| {
                            if let Source::Smart { limit, .. } = s {
                                *limit = n;
                            }
                        });
                        cx.notify();
                    },
                ))
            });
            filters = filters
                .child(v_flex().gap_1p5().child(label("Categories")).child(h_flex().gap_1p5().children(cats)))
                .child(v_flex().gap_1p5().child(label("Sort")).child(h_flex().gap_1p5().children(sorts)))
                .child(v_flex().gap_1p5().child(label("Keep")).child(h_flex().gap_1p5().children(limits)));
        }
        let changed = self.gallery_pop.as_ref().is_some_and(|pop| pop.opened_with != g.source);

        // ---- actions ----
        let rotating = self.cfg.rotate_source == RotateSource::Gallery(id) && self.cfg.rotate_minutes > 0;
        let deleting = self.confirm_delete == Some(id);
        let action = |aid: &'static str, icon_name: &'static str, tip: &'static str, on: bool| {
            div()
                .id(aid)
                .flex_1()
                .h(px(34.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_md()
                .cursor_pointer()
                .bg(if on { with_alpha(c, 0.35) } else { hsla(0., 0., 1., 0.06) })
                .text_color(white())
                .hover(move |s| s.bg(with_alpha(c, 0.3)))
                .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(tip).build(window, cx))
                .child(icon(icon_name).size_4())
        };
        let refreshable = g.source.refreshable();
        let actions = h_flex()
            .gap_1p5()
            .when(refreshable, |d| {
                d.child(action("pop-refresh", "refresh-cw", "Refresh now", false).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.refresh_gallery(id, cx);
                        this.close_gallery_pop(cx);
                    },
                )))
            })
            .child(action("pop-offline", "hard-drive-download", "Keep originals offline", g.offline).on_click(
                cx.listener(move |this, _, _, cx| {
                    let on = this.galleries.get(id).is_some_and(|g| g.offline);
                    this.set_offline(id, !on, cx);
                }),
            ))
            .child(
                action("pop-rotate", "shuffle", "Rotate from this gallery", rotating)
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_rotation(id, cx))),
            )
            .child(action("pop-play", "play", "Put up a set now", false).on_click(cx.listener(
                move |this, _, _, cx| {
                    this.rotate_from(RotateSource::Gallery(id), cx);
                    this.close_gallery_pop(cx);
                },
            )))
            .child(action("pop-rename", "pencil", "Rename", false).on_click(cx.listener(move |this, _, window, cx| {
                this.close_gallery_pop(cx);
                this.open_gallery(id, cx);
                this.start_rename(id, window, cx);
            })))
            .child(action("pop-edit", "sliders-horizontal", "Every setting", false).on_click(cx.listener(
                move |this, _, window, cx| {
                    this.close_gallery_pop(cx);
                    this.open_gallery_editor(id, window, cx);
                },
            )))
            .child(if deleting {
                div()
                    .id("pop-delete")
                    .px_2()
                    .h(px(34.))
                    .flex()
                    .items_center()
                    .rounded_md()
                    .cursor_pointer()
                    .bg(p.error)
                    .text_xs()
                    .text_color(white())
                    .child("Delete?")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.gallery_pop = None;
                        this.delete_gallery(id, cx);
                    }))
            } else {
                action("pop-delete", "trash", "Delete gallery", false).on_click(cx.listener(move |this, _, _, cx| {
                    this.confirm_delete = Some(id);
                    cx.notify();
                }))
            });

        // ---- the card itself: the cover is the whole card ----
        let n = g.items.len();
        let title = v_flex()
            .px_3p5()
            .pt_3()
            .pb_8()
            .bg(linear_gradient(
                180.,
                linear_color_stop(hsla(0., 0., 0., 0.7), 0.),
                linear_color_stop(hsla(0., 0., 0., 0.), 1.),
            ))
            .child(
                div()
                    .text_base()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(white())
                    .truncate()
                    .child(g.name.clone()),
            )
            .child(div().text_xs().text_color(hsla(0., 0., 1., 0.75)).truncate().child(format!(
                "{n} wallpaper{}  ·  {}",
                if n == 1 { "" } else { "s" },
                describe(&g.source)
            )));
        // Filters and actions on frosted glass over the lower part; the
        // picture shows through.
        let glass_panel = v_flex()
            .gap_3()
            .p_3()
            .bg(linear_gradient(
                180.,
                linear_color_stop(Hsla { l: 0.1, s: c.s.min(0.55), a: 0.72, ..c }, 0.),
                linear_color_stop(Hsla { l: 0.06, s: c.s.min(0.4), a: 0.92, ..c }, 1.),
            ))
            .border_t_1()
            .border_color(with_alpha(c, 0.45))
            .child(filters)
            .when(changed, |d| {
                d.child(
                    h_flex()
                        .gap_1p5()
                        .text_xs()
                        .text_color(with_alpha(white(), 0.7))
                        .child(icon("refresh-cw").size_3())
                        .child("Refreshes with these when you close this"),
                )
            })
            .child(actions);

        let card = v_flex()
            .id("gallery-pop")
            .absolute()
            // Clipped to its growing box, so nothing shows outside it mid-flight.
            .overflow_hidden()
            .rounded(px(R))
            .bg(p.bg)
            .shadow_lg()
            .occlude()
            .child(div().absolute().inset_0().when_some(cover, |d, t| {
                d.child(radius(box_ratio(img(t).size_full(), POP_W, pop_h).object_fit(ObjectFit::Cover), R))
            }))
            .child(div().relative().child(title))
            .child(div().flex_1())
            .child(div().relative().map(|d| super::radius_bottom(d, R)).overflow_hidden().child(glass_panel))
            // Its edge in its own color, on top.
            .child(div().absolute().inset_0().rounded(px(R)).border_1().border_color(with_alpha(c, 0.7)))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            // Grow out of the tile, and settle back into it.
            .with_animation(
                SharedString::from(format!("gallery-pop-{}-{seq}", if closing { "settle" } else { "lift" })),
                Animation::new(Duration::from_millis(if closing { CLOSE_MS } else { OPEN_MS })).with_easing({
                    let quint = ease_out_quint();
                    move |t| if closing { ease_in_out(t) } else { quint(t) }
                }),
                move |d, t| {
                    let t = if closing { 1. - t } else { t };
                    let lerp = |a: f32, b: f32| a + (b - a) * t;
                    d.left(px(lerp(fx, tx)))
                        .top(px(lerp(fy, ty)))
                        .w(px(lerp(fw, POP_W)))
                        .h(px(lerp(fh, pop_h)))
                        .when(closing, |d| d.opacity(0.35 + 0.65 * t))
                },
            );

        Some(
            deferred(
                anchored().position(point(px(0.), px(0.))).child(
                    div()
                        .id("gallery-pop-stage")
                        .relative()
                        .w(px(vw))
                        .h(px(vh))
                        .occlude()
                        .child(div().absolute().inset_0().bg(hsla(0., 0., 0., 0.3)).with_animation(
                            SharedString::from(format!("gallery-pop-dim-{seq}-{closing}")),
                            Animation::new(Duration::from_millis(if closing { CLOSE_MS } else { OPEN_MS })),
                            move |d, t| d.opacity(if closing { 1. - t } else { t }),
                        ))
                        .child(card)
                        .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.close_gallery_pop(cx)))
                        .on_mouse_down(MouseButton::Right, cx.listener(|this, _, _, cx| this.close_gallery_pop(cx))),
                ),
            )
            .with_priority(3)
            .into_any_element(),
        )
    }
}

fn describe(source: &Source) -> String {
    match source {
        Source::Manual => "hand-picked".into(),
        Source::Smart { search, .. } => search.describe(),
        Source::Wallhaven { user, .. } => format!("@{user}'s collection"),
    }
}
