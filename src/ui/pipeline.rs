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

use super::span_editor::ComposeMode;
use super::{
    LockMode, Muralist, Pal, Reach, Target, TargetMode, boxed, drag, hex, icon, kit, on_accent, pal, radius, segment,
    segmented_track, with_alpha,
};

/// Corner radius of a screen tile, shared by its picture and its edge.
const TILE_R: f32 = 8.;
/// Height available to the monitor layout.
const LAYOUT_H: f32 = 92.;
/// Width of the rail column.
const RAIL_W: f32 = 24.;
/// How far a step's content sits in from the panel's edge: rail plus gap.
pub(super) const STEP_INDENT: f32 = RAIL_W + 12.;

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
            .when(!last, gpui_kit::Styled::pb_6)
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

    /// Throws the plan away: back to what's actually up.
    pub(super) fn discard_staged(&mut self, cx: &mut Context<Self>) {
        self.desktop = self.applied.desktop.clone();
        self.lock = self.applied.lock.clone();
        self.landing.clear();
        self.stage_gen += 1;
        self.set_status("Plan cleared", false, cx);
        cx.notify();
    }

    /// Width and height of the whole monitor arrangement, in desktop units.
    fn desk_span(&self) -> (f32, f32) {
        let (min_x, min_y, max_x, max_y) = crate::monitors::extent(&self.monitors);
        (((max_x - min_x) as f32).max(1.), ((max_y - min_y) as f32).max(1.))
    }

    /// Monitors scaled to fit `max_w` x `max_h`; clicking or dropping on a
    /// screen stages a wallpaper there for `target`.
    fn render_layout(&mut self, target: Target, max_w: f32, max_h: f32, cx: &mut Context<Self>) -> AnyElement {
        let p = pal();
        if self.monitors.is_empty() {
            return div().text_sm().text_color(p.muted).child("No displays detected").into_any_element();
        }
        let (min_x, min_y, _, _) = crate::monitors::extent(&self.monitors);
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
                    .when(has_selection, gpui_kit::Styled::cursor_pointer)
                    .when_some(has_selection.then_some(()), |d, _| {
                        d.tooltip(|window, cx| Tooltip::new("Put the selected wallpaper here").build(window, cx))
                    })
                    // The picture, rounded to the tile's own radius.
                    .map(|d| match thumb {
                        // Inset under the edge, so the picture's soft corner
                        // can't show past the edge's own.
                        Some(t) => d.child(
                            // Sized to the tile exactly (see `boxed`).
                            div().absolute().top(px(1.)).left(px(1.)).w(px(tile_w - 2.)).h(px(tile_h - 2.)).child(
                                radius(
                                    boxed(img(t), tile_w - 2., tile_h - 2.).object_fit(ObjectFit::Cover),
                                    TILE_R - 1.,
                                ),
                            ),
                        ),
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
                    // Dropping a card on a screen, or clicking one, opens the
                    // composer for just that screen.
                    .on_drop(cx.listener({
                        let name = name.clone();
                        move |this, drag: &drag::CardDrag, window, cx| {
                            this.select(drag.item.clone(), window, cx);
                            this.open_composer(
                                ComposeMode::One,
                                Some(name.clone()),
                                super::span_editor::ComposeExit::Plan,
                                cx,
                            );
                        }
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let _ = target;
                        this.open_composer(
                            ComposeMode::One,
                            Some(name.clone()),
                            super::span_editor::ComposeExit::Plan,
                            cx,
                        );
                    }))
            })
            .collect();

        div().relative().flex_none().w(px(span_w * scale)).h(px(span_h * scale)).children(screens).into_any_element()
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
        // Info and Similar open in a modal, from two little dossiers.
        let toggles = selected.as_ref().map(|item| self.render_dossier(item, cx));
        // Or let muralist choose what goes up: Spin picks at random from your
        // results, Match picks ones whose colors suit the selection.
        let can_spin = !self.results.is_empty() && self.slot.is_none();
        let choose = |id: &'static str, icon_name: &'static str, label: &'static str, tip: &'static str, on: bool| {
            kit::Btn::new(id).label(label).icon(icon_name).sm().disabled(!on).tooltip(tip).render().flex_1()
        };
        let match_tip = if has_selection {
            "Fill the other screens with wallpapers whose colors match this one"
        } else {
            "Fill every screen with wallpapers whose colors match each other"
        };
        let pickers = h_flex()
            .gap_2()
            .child(
                choose("spin", "dices", "Spin", "A random wallpaper for every screen, from your results", can_spin)
                    .on_click(cx.listener(|this, _, _, cx| this.spin(cx))),
            )
            .child(
                choose("harmonize", "palette", "Match", match_tip, can_spin)
                    .on_click(cx.listener(|this, _, _, cx| this.harmonize(cx))),
            );
        let choose_hint =
            if has_selection { "Or let muralist choose" } else { "Pick one from the grid, or let muralist choose" };
        let wallpaper = v_flex()
            .gap_3()
            .child(details)
            .children(toggles)
            .child(v_flex().gap_1p5().child(div().text_xs().text_color(p.dim).child(choose_hint)).child(pickers))
            .into_any_element();

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
                                                this.set_lock_mode(LockMode::PerMonitor, cx);
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
        let avail = self.preview_w();
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
        // Copying between surfaces sits with the rows it moves pictures
        // between: arrows between them in Both, a link under the row
        // otherwise. It goes into the plan, like any other arrangement.
        let copy_btn = |id: &'static str, icon_name: &'static str, label: &'static str, from: Target| {
            let has = !match from {
                Target::Desktop => &self.applied.desktop,
                Target::Lock => &self.applied.lock,
            }
            .is_empty();
            kit::Btn::new(id).icon(icon_name).label(label).ghost().sm().disabled(!has).on_click(cx.listener(
                move |this, _, _, cx| {
                    match from {
                        Target::Desktop => this.lock = this.applied.desktop.clone(),
                        Target::Lock => this.desktop = this.applied.lock.clone(),
                    }
                    cx.notify();
                },
            ))
        };
        let copies = lock_reach.is_some().then(|| match mode {
            TargetMode::Both => h_flex()
                .justify_center()
                .gap_1()
                .child(copy_btn("copy-down", "arrow-down", "Desktop to lock", Target::Desktop))
                .child(copy_btn("copy-up", "arrow-up", "Lock to desktop", Target::Lock)),
            TargetMode::Desktop => {
                h_flex().justify_end().child(copy_btn("copy-up", "lock", "Use the lock screen's", Target::Lock))
            }
            TargetMode::Lock => {
                h_flex().justify_end().child(copy_btn("copy-down", "monitor", "Use the desktop's", Target::Desktop))
            }
        });
        let layouts = if stacked && layouts.len() == 2 {
            let mut rows = layouts.into_iter();
            v_flex().items_center().gap_1().children(rows.next()).children(copies).children(rows.next())
        } else if stacked {
            v_flex().items_center().gap_2().children(layouts).children(copies)
        } else {
            v_flex().gap_1().child(h_flex().justify_center().gap_3().children(layouts)).children(copies)
        };

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
        // What to do with the selection; nothing to show without one.
        let tiles = if !has_selection {
            Vec::new()
        } else {
            vec![
                tile(
                    "fill-all",
                    "layout-dashboard",
                    "Every screen",
                    "Put the selected wallpaper on every screen, framing each as you like",
                    has_selection,
                )
                .when(has_selection, |d| {
                    d.on_click(cx.listener(|this, _, _, cx| {
                        this.open_composer(ComposeMode::Every, None, super::span_editor::ComposeExit::Plan, cx)
                    }))
                })
                .into_any_element(),
                tile("span", "frame", "Span", "One picture across every screen", has_selection)
                    .when(has_selection, |d| {
                        d.on_click(cx.listener(|this, _, _, cx| {
                            this.open_span_editor(super::span_editor::ComposeExit::Plan, cx)
                        }))
                    })
                    .into_any_element(),
            ]
        };
        let cols = tiles.len().max(1) as u16;
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
                .child(format!("{changes} to set"))
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
                "Click a screen (or drop a card on one) to arrange it, or use Every screen or Span."
            } else if staged {
                "Your plan is ready: Set below puts it up."
            } else {
                "Pick a wallpaper to arrange it on your screens."
            }))
            .when(!tiles.is_empty(), |d| d.child(div().grid().grid_cols(cols).gap_2().children(tiles)))
            .into_any_element();
        let s3 = if staged {
            StepState::Done
        } else if has_selection {
            StepState::Current
        } else {
            StepState::Waiting
        };

        // ---- 4. Set ---------------------------------------------------------
        // Puts up exactly what step 3 shows.
        let apply_body = if staged {
            h_flex()
                .gap_2()
                .child(
                    div().flex_1().child(
                        kit::Btn::new("apply")
                            .label(if fetching > 0 {
                                format!("Fetching {fetching}")
                            } else {
                                format!("Set {changes} screen{}", if changes == 1 { "" } else { "s" })
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
                        .tooltip("Throw the plan away: back to what's up now")
                        .on_click(cx.listener(|this, _, _, cx| this.discard_staged(cx))),
                )
                .into_any_element()
        } else {
            h_flex()
                .gap_1p5()
                .text_sm()
                .text_color(p.dim)
                .child(icon("circle-check").size_4().text_color(p.ok))
                .child(if has_selection {
                    "Arrange it in Screens above, then Set it here"
                } else {
                    "Your screens are up to date"
                })
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
            .child(step(4, "Set", s4, true, None, apply, p))
            .into_any_element()
    }
}
