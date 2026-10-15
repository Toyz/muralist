//! The right side, as one flow you read top to bottom:
//!
//! 1. Wallpaper: what you picked (preview, details, Info and Similar).
//! 2. Where: Desktop, Lock screen, or Both.
//! 3. Screens: your monitors; click one, or Fill all / Span / Spin / Match.
//! 4. Apply: commit what's staged, or discard it.
//!
//! A rail of numbered markers ties the steps together; each lights up as
//! its step is done. Every control sits next to what it changes.

use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    InspectorTab, Item, LockMode, Muralist, Pal, Placement, Reach, Target, TargetMode, details, drag, hex, icon, kit,
    on_accent, pal, radius, section, segment, segmented_track, with_alpha,
};

/// Corner radius of a screen tile, shared by its picture and its edge.
const TILE_R: f32 = 8.;
/// Height available to the monitor layout.
const LAYOUT_H: f32 = 92.;
/// Width of the rail column.
const RAIL_W: f32 = 24.;

#[derive(Clone, Copy, PartialEq, Eq)]
enum StepState {
    /// Not reached yet.
    Waiting,
    /// Where you are.
    Current,
    /// Done.
    Done,
}

/// One step: a numbered marker on the rail, a title, and its body.
fn step(
    n: u8,
    title: &str,
    state: StepState,
    last: bool,
    accessory: Option<AnyElement>,
    body: AnyElement,
    p: Pal,
) -> Div {
    let marker = div()
        .flex_none()
        .size(px(22.))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .map(|d| match state {
            StepState::Done => d.bg(p.accent).text_color(on_accent(p.accent)).child(icon("check").size_3()),
            StepState::Current => d
                .border_2()
                .border_color(p.accent)
                .text_color(p.accent)
                .bg(with_alpha(p.accent, 0.12))
                .child(n.to_string()),
            StepState::Waiting => d.border_1().border_color(p.border_strong).text_color(p.dim).child(n.to_string()),
        });
    let rail = v_flex().flex_none().w(px(RAIL_W)).items_center().child(marker).when(!last, |d| {
        d.child(div().w(px(2.)).flex_1().mt_1().rounded_full().bg(if state == StepState::Done {
            with_alpha(p.accent, 0.55)
        } else {
            p.border
        }))
    });
    div().flex().flex_row().gap_3().child(rail).child(
        v_flex()
            .flex_1()
            .min_w_0()
            .gap_3()
            .when(!last, |d| d.pb_6())
            .child(
                h_flex()
                    .h(px(22.))
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(if state == StepState::Waiting { p.dim } else { p.muted })
                            .child(title.to_uppercase()),
                    )
                    .children(accessory),
            )
            .child(body),
    )
}

impl Muralist {
    /// Screens whose staged image differs from what's up, for the chosen
    /// surfaces.
    fn pending_changes(&self) -> usize {
        self.target
            .targets()
            .iter()
            .map(|t| {
                let (pending, applied) = match t {
                    Target::Desktop => (&self.desktop, &self.applied.desktop),
                    Target::Lock => (&self.lock, &self.applied.lock),
                };
                pending.iter().filter(|(k, v)| applied.get(*k) != Some(*v)).count()
            })
            .sum()
    }

    /// Puts the staged screens back to what's actually up.
    pub(super) fn discard_staged(&mut self, cx: &mut Context<Self>) {
        self.desktop = self.applied.desktop.clone();
        self.lock = self.applied.lock.clone();
        self.landing.clear();
        self.stage_gen += 1;
        self.set_status("Staged changes discarded", false, cx);
        cx.notify();
    }

    /// Width and height of the whole monitor arrangement, in desktop units.
    fn desk_span(&self) -> (f32, f32) {
        let min_x = self.monitors.iter().map(|m| m.x).min().unwrap_or(0);
        let min_y = self.monitors.iter().map(|m| m.y).min().unwrap_or(0);
        let max_x = self.monitors.iter().map(|m| m.x + m.width as i32).max().unwrap_or(1);
        let max_y = self.monitors.iter().map(|m| m.y + m.height as i32).max().unwrap_or(1);
        (((max_x - min_x) as f32).max(1.), ((max_y - min_y) as f32).max(1.))
    }

    /// Monitors scaled to fit `max_w` x `max_h`; clicking or dropping on a
    /// screen stages a wallpaper there for `target`.
    fn render_layout(&mut self, target: Target, max_w: f32, max_h: f32, cx: &mut Context<Self>) -> AnyElement {
        let p = pal();
        if self.monitors.is_empty() {
            return div().text_sm().text_color(p.muted).child("No displays detected").into_any_element();
        }
        let min_x = self.monitors.iter().map(|m| m.x).min().unwrap();
        let min_y = self.monitors.iter().map(|m| m.y).min().unwrap();
        let (span_w, span_h) = self.desk_span();
        let scale = (max_w / span_w).min(max_h / span_h);
        let gap = 6.;

        let (pending, applied) = match target {
            Target::Desktop => (self.desktop.clone(), self.applied.desktop.clone()),
            Target::Lock => (self.lock.clone(), self.applied.lock.clone()),
        };
        let has_selection = self.selected.is_some();
        let dragging = cx.has_active_drag();

        let screens: Vec<_> = self
            .monitors
            .clone()
            .into_iter()
            .map(|m| {
                let path = pending.get(&m.name).cloned();
                let changed = path != applied.get(&m.name).cloned();
                let reel = self.reel_face(&m.name);
                let glow_c = path.as_deref().and_then(|p| self.path_glow(p));
                let thumb = match &reel {
                    Some((w, _)) => self.remote_thumb(w, false),
                    None => path.as_deref().and_then(|p| self.local_thumb(p)),
                };
                let name = m.name.clone();
                let (tile_w, tile_h) = (m.width as f32 * scale - gap, m.height as f32 * scale - gap);
                // Small tiles keep only the picture; labels need room.
                let roomy = tile_w >= 72. && tile_h >= 34.;
                div()
                    .id(SharedString::from(format!("mon-{}-{}", target.label(), m.name)))
                    .absolute()
                    .left(px((m.x - min_x) as f32 * scale + gap / 2.))
                    .top(px((m.y - min_y) as f32 * scale + gap / 2.))
                    .w(px(tile_w))
                    .h(px(tile_h))
                    .map(|d| radius(d, TILE_R))
                    .bg(p.bg)
                    .when(has_selection, |d| d.cursor_pointer())
                    .when_some(has_selection.then_some(()), |d, _| {
                        d.tooltip(|window, cx| Tooltip::new("Put the selected wallpaper here").build(window, cx))
                    })
                    // The picture, rounded to the tile's own radius.
                    .map(|d| match thumb {
                        Some(t) => d.child(radius(img(t).size_full().object_fit(ObjectFit::Cover), TILE_R)),
                        None => d.child(
                            div()
                                .size_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon("plus").size_4().text_color(p.dim)),
                        ),
                    })
                    .when_some(glow_c.filter(|_| reel.is_none() && roomy), |d, c| {
                        d.child(
                            div()
                                .absolute()
                                .bottom_0()
                                .left(px(TILE_R))
                                .right(px(TILE_R))
                                .h(px(2.))
                                .bg(with_alpha(c, 0.85)),
                        )
                    })
                    .when_some(reel.clone(), |d, (w, stopped)| {
                        if stopped {
                            // The reel lands: one bright flash.
                            d.child(div().absolute().inset_0().map(|d| radius(d, TILE_R)).bg(white()).with_animation(
                                SharedString::from(format!("reel-stop-{}-{}", m.name, w.id)),
                                Animation::new(std::time::Duration::from_millis(500)).with_easing(ease_out_quint()),
                                |d, t| d.opacity(0.7 * (1. - t)),
                            ))
                        } else {
                            // Spinning: darkened reel window edges.
                            d.child(div().absolute().inset_0().map(|d| radius(d, TILE_R)).bg(linear_gradient(
                                180.,
                                linear_color_stop(hsla(0., 0., 0., 0.55), 0.),
                                linear_color_stop(hsla(0., 0., 0., 0.), 0.5),
                            )))
                        }
                    })
                    .when(roomy, |d| {
                        d.child(
                            h_flex()
                                .absolute()
                                .bottom(px(5.))
                                .left(px(5.))
                                .gap_1()
                                .px_1()
                                .rounded_sm()
                                .bg(hsla(0., 0., 0., 0.55))
                                .text_size(px(10.))
                                .text_color(hsla(0., 0., 1., 0.92))
                                .child(m.name.clone())
                                .when(m.primary, |d| d.child(icon("star-fill").size(px(8.)).text_color(hex(0xfbbf24)))),
                        )
                    })
                    .when(changed, |d| {
                        d.child(
                            div()
                                .absolute()
                                .top(px(5.))
                                .right(px(5.))
                                .size(px(7.))
                                .rounded_full()
                                .bg(p.accent)
                                .border_1()
                                .border_color(hsla(0., 0., 0., 0.45)),
                        )
                    })
                    // The edge goes on last, so the picture can't paint over it.
                    .child(div().absolute().inset_0().map(|d| radius(d, TILE_R)).border_2().border_color(
                        if changed || reel.is_some() {
                            p.accent
                        } else if dragging {
                            with_alpha(p.accent, 0.55)
                        } else {
                            p.border_strong
                        },
                    ))
                    .when(has_selection, |d| d.hover(move |s| s.opacity(0.9)))
                    .drag_over::<drag::CardDrag>(move |s, _, _, _| s.opacity(0.75))
                    .on_drop(cx.listener({
                        let name = name.clone();
                        move |this, drag: &drag::CardDrag, window, cx| {
                            this.select(drag.item.clone(), window, cx);
                            this.assign_item(drag.item.clone(), vec![target], Placement::Monitor(name.clone()), cx);
                        }
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.assign(vec![target], Placement::Monitor(name.clone()), cx)
                    }))
            })
            .collect();

        div().relative().flex_none().w(px(span_w * scale)).h(px(span_h * scale)).children(screens).into_any_element()
    }

    /// Info or Similar for the selection, when one is open.
    fn render_more(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = pal();
        let item = self.selected.clone()?;
        match (self.inspector_tab, &item) {
            (InspectorTab::Info, Item::Remote(w)) => {
                let w = w.clone();
                Some(self.render_info_tab(&w, cx))
            }
            (InspectorTab::Info, Item::Local(path)) => {
                let path = path.clone();
                let palette = self.render_xray_palette(cx);
                Some(
                    v_flex()
                        .gap_5()
                        .children(palette)
                        .child(
                            section("File", p)
                                .child(div().text_xs().text_color(p.muted).child(path.display().to_string())),
                        )
                        .into_any_element(),
                )
            }
            (InspectorTab::Similar, Item::Remote(w)) => {
                let w = w.clone();
                Some(self.render_similar(&w, cx).unwrap_or_else(|| {
                    div().text_sm().text_color(p.dim).child("No similar wallpapers found").into_any_element()
                }))
            }
            _ => None,
        }
    }

    /// The inspector's body: the four steps.
    pub(super) fn render_pipeline(&mut self, details: AnyElement, cx: &mut Context<Self>) -> AnyElement {
        let p = pal();
        let selected = self.selected.clone();
        let has_selection = selected.is_some();
        let supported = self.backend.is_supported() && !self.monitors.is_empty();
        let changes = self.pending_changes();
        let fetching = self.landing.len();
        let busy = self.busy > 0;
        let staged = changes > 0 || fetching > 0 || busy;

        // ---- 1. Wallpaper ----------------------------------------------------
        let open = self.inspector_tab;
        let similar_count = match &selected {
            Some(Item::Remote(w)) => match self.similar.get(&w.id) {
                Some(details::Similar::Ready(list)) => Some(list.len()),
                _ => None,
            },
            _ => None,
        };
        // Info and Similar fold open under the wallpaper; click again to close.
        let more_toggle = |id: &'static str, label: String, icon_name: &'static str, t: InspectorTab| {
            let on = open == t;
            kit::Btn::new(id).label(label).icon(icon_name).sm().ghost().selected(on).on_click(cx.listener(
                move |this, _, _, cx| {
                    this.inspector_tab = if this.inspector_tab == t { InspectorTab::Set } else { t };
                    cx.notify();
                },
            ))
        };
        let toggles = selected.as_ref().map(|item| {
            let remote = matches!(item, Item::Remote(_));
            h_flex().gap_1p5().child(more_toggle("more-info", "Info".into(), "info", InspectorTab::Info)).when(
                remote,
                |d| {
                    d.child(more_toggle(
                        "more-similar",
                        match similar_count {
                            Some(n) if n > 0 => format!("Similar  {n}"),
                            _ => "Similar".into(),
                        },
                        "images",
                        InspectorTab::Similar,
                    ))
                },
            )
        });
        let more = self.render_more(cx);
        let wallpaper = v_flex().gap_3().child(details).children(toggles).children(more).into_any_element();

        let s1 = if has_selection { StepState::Done } else { StepState::Current };
        if !supported {
            let note = h_flex()
                .gap_2()
                .text_sm()
                .text_color(p.muted)
                .child(icon("triangle-alert").size_4().text_color(hex(0xf59e0b)))
                .child(if self.backend.is_supported() {
                    "No displays detected.".to_owned()
                } else {
                    format!(
                        "muralist can't set wallpapers on {} yet. You can still browse and download.",
                        self.backend.name()
                    )
                })
                .into_any_element();
            return v_flex()
                .child(step(1, "Wallpaper", s1, false, None, wallpaper, p))
                .child(step(2, "Screens", StepState::Waiting, true, None, note, p))
                .into_any_element();
        }

        // ---- 2. Where -------------------------------------------------------
        let lock_reach = self.backend.lock().map(|w| w.reach());
        if lock_reach.is_none() {
            self.target = TargetMode::Desktop;
        }
        let mode = self.target;
        let where_btn = |id: &'static str, icon_name: &'static str, label: &'static str, m: TargetMode| {
            let on = mode == m;
            h_flex()
                .id(id)
                .flex_1()
                .h(px(34.))
                .justify_center()
                .gap_1p5()
                .rounded_md()
                .text_sm()
                .cursor_pointer()
                .map(|d| {
                    if on {
                        d.bg(p.elevated).text_color(p.fg).font_weight(FontWeight::MEDIUM).shadow_sm()
                    } else {
                        d.text_color(p.muted).hover(move |s| s.text_color(p.fg))
                    }
                })
                .child(icon(icon_name).size_3p5().text_color(if on { p.accent } else { p.dim }))
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.target = m;
                    cx.notify();
                }))
        };
        let lock_mode = self.cfg.lock_mode;
        let where_body = match lock_reach {
            None => div()
                .text_sm()
                .text_color(p.muted)
                .child("Desktop. This desktop doesn't let muralist set the lock screen.")
                .into_any_element(),
            Some(reach) => v_flex()
                .gap_2()
                .child(
                    segmented_track(p)
                        .child(where_btn("where-desktop", "monitor", "Desktop", TargetMode::Desktop))
                        .child(where_btn("where-lock", "lock", "Lock screen", TargetMode::Lock))
                        .child(where_btn("where-both", "layers", "Both", TargetMode::Both)),
                )
                .when(mode != TargetMode::Desktop && reach == Reach::PerMonitor, |d| {
                    d.child(
                        h_flex()
                            .justify_between()
                            .gap_2()
                            .child(div().text_xs().text_color(p.dim).child("Lock screen shows"))
                            .child(
                                segmented_track(p)
                                    .child(
                                        segment("lm-per", "A picture per screen", lock_mode == LockMode::PerMonitor, p)
                                            .px_2()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.set_lock_mode(LockMode::PerMonitor, cx)
                                            })),
                                    )
                                    .child(
                                        segment("lm-single", "One image", lock_mode == LockMode::Single, p)
                                            .px_2()
                                            .on_click(
                                                cx.listener(|this, _, _, cx| this.set_lock_mode(LockMode::Single, cx)),
                                            ),
                                    ),
                            ),
                    )
                })
                .into_any_element(),
        };
        let s2 = if has_selection || staged { StepState::Done } else { StepState::Waiting };

        // ---- 3. Screens -----------------------------------------------------
        let targets = mode.targets();
        let avail = self.details_base() - RAIL_W - 12.;
        let (span_w, span_h) = self.desk_span();
        // Wide desks stack Desktop over Lock; others sit side by side.
        let stacked = targets.len() > 1 && span_w / span_h > 1.8;
        let label_w = if targets.len() > 1 { 20. } else { 0. };
        let (box_w, box_h) = match (targets.len(), stacked) {
            (1, _) => (avail, LAYOUT_H),
            (_, true) => (avail - label_w, 58.),
            (_, false) => ((avail - 12.) / 2. - label_w, LAYOUT_H),
        };
        let layouts: Vec<AnyElement> = targets
            .iter()
            .map(|&t| {
                let layout = self.render_layout(t, box_w, box_h, cx);
                h_flex()
                    .gap_1()
                    .when(targets.len() > 1, |d| {
                        d.child(div().w(px(label_w)).flex().justify_center().child(
                            icon(if t == Target::Lock { "lock" } else { "monitor" }).size_3p5().text_color(p.dim),
                        ))
                    })
                    .child(layout)
                    .into_any_element()
            })
            .collect();
        let layouts = if stacked {
            v_flex().items_center().gap_2().children(layouts)
        } else {
            h_flex().justify_center().gap_3().children(layouts)
        };

        let can_spin = !self.results.is_empty() && self.slot.is_none();
        let tile = |id: &'static str, name: &'static str, label: &'static str, tip: &'static str, enabled: bool| {
            kit::surface(v_flex(), p)
                .id(id)
                .items_center()
                .justify_center()
                .gap_1()
                .h(px(54.))
                .rounded_lg()
                .text_xs()
                .text_color(p.muted)
                .child(icon(name).size_4().text_color(if enabled { p.accent } else { p.dim }))
                .child(label)
                .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
                .map(|d| {
                    if enabled {
                        d.cursor_pointer()
                            .hover(move |s| {
                                s.bg(with_alpha(p.accent, 0.12))
                                    .border_color(with_alpha(p.accent, 0.6))
                                    .text_color(p.fg)
                            })
                            .active(|s| s.pt(px(2.)))
                    } else {
                        d.opacity(0.4)
                    }
                })
        };
        let mut tiles = vec![
            tile(
                "fill-all",
                "layout-dashboard",
                "Fill all",
                "Put the selected wallpaper on every screen",
                has_selection,
            )
            .when(has_selection, |d| {
                d.on_click(cx.listener(|this, _, _, cx| this.assign(this.target.targets(), Placement::All, cx)))
            })
            .into_any_element(),
            tile("span", "frame", "Span", "Stretch the selected wallpaper across every screen", has_selection)
                .when(has_selection, |d| d.on_click(cx.listener(|this, _, _, cx| this.open_span_editor(cx))))
                .into_any_element(),
            tile("spin", "dices", "Spin", "Random wallpapers from your results, slot-machine style", can_spin)
                .when(can_spin, |d| d.on_click(cx.listener(|this, _, _, cx| this.spin(cx))))
                .into_any_element(),
            tile("harmonize", "palette", "Match", "Wallpapers whose colors match the selected one", can_spin)
                .when(can_spin, |d| d.on_click(cx.listener(|this, _, _, cx| this.harmonize(cx))))
                .into_any_element(),
        ];
        if mode == TargetMode::Lock {
            tiles.push(
                tile("copy-desktop", "copy", "Copy", "Use the desktop's wallpapers on the lock screen", true)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.lock = this.desktop.clone();
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }
        let cols = tiles.len() as u16;
        let screen_status: AnyElement = if fetching > 0 {
            h_flex()
                .gap_1()
                .text_xs()
                .text_color(p.accent)
                .child(icon("download").size_3())
                .child(format!("fetching {fetching}"))
                .into_any_element()
        } else if changes > 0 {
            h_flex()
                .gap_1p5()
                .text_xs()
                .text_color(p.accent)
                .child(div().size(px(6.)).rounded_full().bg(p.accent))
                .child(format!("{changes} staged"))
                .into_any_element()
        } else {
            h_flex()
                .gap_1()
                .text_xs()
                .text_color(p.dim)
                .child(icon("circle-check").size_3().text_color(p.ok))
                .child("up to date")
                .into_any_element()
        };
        let screens = v_flex()
            .gap_3()
            .child(layouts)
            .child(div().text_xs().text_color(p.dim).child(if has_selection {
                "Click a screen to put the wallpaper there, or drag a card onto one."
            } else {
                "Pick a wallpaper, or let Spin and Match choose for you."
            }))
            .child(div().grid().grid_cols(cols).gap_2().children(tiles))
            .into_any_element();
        let s3 = if staged {
            StepState::Done
        } else if has_selection {
            StepState::Current
        } else {
            StepState::Waiting
        };

        // ---- 4. Apply -------------------------------------------------------
        let apply_body = if staged {
            h_flex()
                .gap_2()
                .child(
                    div().flex_1().child(
                        kit::Btn::new("apply")
                            .label(if fetching > 0 {
                                format!("Fetching {fetching}")
                            } else {
                                format!("Apply {changes} change{}", if changes == 1 { "" } else { "s" })
                            })
                            .icon("check")
                            .primary()
                            .lg()
                            .full()
                            .loading(busy)
                            .on_click(cx.listener(|this, _, _, cx| this.apply(this.target.targets(), cx))),
                    ),
                )
                .child(
                    kit::Btn::new("discard")
                        .icon("undo-2")
                        .lg()
                        .tooltip("Discard staged changes")
                        .on_click(cx.listener(|this, _, _, cx| this.discard_staged(cx))),
                )
                .into_any_element()
        } else if has_selection {
            v_flex()
                .gap_1p5()
                .child(
                    kit::Btn::new("set-now")
                        .label("Set as wallpaper")
                        .icon("wallpaper")
                        .primary()
                        .lg()
                        .full()
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(item) = this.selected.clone() {
                                this.assign_item_then(item, this.target.targets(), Placement::All, true, cx);
                            }
                        })),
                )
                .child(div().text_xs().text_color(p.dim).child("Fills every screen and applies it in one go."))
                .into_any_element()
        } else {
            h_flex()
                .gap_1p5()
                .text_sm()
                .text_color(p.dim)
                .child(icon("circle-check").size_4().text_color(p.ok))
                .child("Your screens are up to date")
                .into_any_element()
        };
        let apply = v_flex()
            .gap_2()
            .child(apply_body)
            .when_some(self.rotation_countdown(), |d, countdown| {
                d.child(
                    h_flex()
                        .gap_1p5()
                        .text_xs()
                        .text_color(p.dim)
                        .child(icon("shuffle").size_3())
                        .child(format!("Auto-rotating every {} min  ·  {countdown}", self.cfg.rotate_minutes)),
                )
            })
            .into_any_element();
        let s4 = if staged { StepState::Current } else { StepState::Waiting };

        v_flex()
            .child(step(1, "Wallpaper", s1, false, None, wallpaper, p))
            .child(step(2, "Where", s2, false, None, where_body, p))
            .child(step(3, "Screens", s3, false, Some(screen_status), screens, p))
            .child(step(4, "Apply", s4, true, None, apply, p))
            .into_any_element()
    }
}
