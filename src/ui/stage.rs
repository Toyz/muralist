//! The screens stage, docked under the grid: your monitors laid out like the
//! real desk, what's staged on them, and the actions that are about the
//! screens rather than any one wallpaper (Spin, Match, Apply). The
//! inspector's "Use" panel puts the selected wallpaper onto them.

use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    CARD_PAD, GRID_PAD, Item, LockMode, Muralist, Placement, Reach, Target, TargetMode, drag, hex, icon, kit, pal,
    radius, segment, segmented_track, with_alpha,
};

/// Height of the monitor layout inside the stage.
const LAYOUT_H: f32 = 104.;
/// Side columns of the stage: status on the left, actions on the right.
const STAGE_SIDE_W: f32 = 196.;

impl Muralist {
    /// Monitors scaled to fit `max_w` x `max_h`; clicking or dropping on a
    /// screen stages a wallpaper there for `target`.
    pub(super) fn render_layout(
        &mut self,
        target: Target,
        max_w: f32,
        max_h: f32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let p = pal();
        if self.monitors.is_empty() {
            return div().text_color(p.muted).child("No displays detected").into_any_element();
        }
        let min_x = self.monitors.iter().map(|m| m.x).min().unwrap();
        let min_y = self.monitors.iter().map(|m| m.y).min().unwrap();
        let max_x = self.monitors.iter().map(|m| m.x + m.width as i32).max().unwrap();
        let max_y = self.monitors.iter().map(|m| m.y + m.height as i32).max().unwrap();
        let scale = (max_w / (max_x - min_x) as f32).min(max_h / (max_y - min_y) as f32);
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
                div()
                    .id(SharedString::from(format!("mon-{}-{}", target.label(), m.name)))
                    .absolute()
                    .left(px((m.x - min_x) as f32 * scale + gap / 2.))
                    .top(px((m.y - min_y) as f32 * scale + gap / 2.))
                    .w(px(m.width as f32 * scale - gap))
                    .h(px(m.height as f32 * scale - gap))
                    .rounded_lg()
                    .overflow_hidden()
                    .bg(p.bg)
                    .border_2()
                    .border_color(if changed { p.accent } else { p.border })
                    .when(has_selection, |d| d.cursor_pointer().hover(move |s| s.border_color(p.accent_hover)))
                    .when_some(has_selection.then_some(()), |d, _| {
                        d.tooltip(|window, cx| Tooltip::new("Put the selected wallpaper here").build(window, cx))
                    })
                    .map(|d| match thumb {
                        Some(t) => d.child(radius(img(t).size_full().object_fit(ObjectFit::Cover), 6.)),
                        None => d.child(
                            div()
                                .size_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(icon("plus").size_4().text_color(p.dim)),
                        ),
                    })
                    // The image's own color as a crisp strip along the bottom.
                    .when_some(glow_c.filter(|_| reel.is_none()), |d, c| {
                        d.child(div().absolute().bottom_0().left_0().right_0().h(px(3.)).bg(c))
                    })
                    .when_some(reel.clone(), |d, (w, stopped)| {
                        let d = d.border_color(p.accent);
                        if stopped {
                            // The reel lands: one bright flash.
                            d.child(div().absolute().top_0().left_0().size_full().bg(white()).with_animation(
                                SharedString::from(format!("reel-stop-{}-{}", m.name, w.id)),
                                Animation::new(std::time::Duration::from_millis(500)).with_easing(ease_out_quint()),
                                |d, t| d.opacity(0.7 * (1. - t)),
                            ))
                        } else {
                            // Spinning: darkened reel window edges.
                            d.child(div().absolute().top_0().left_0().size_full().bg(linear_gradient(
                                180.,
                                linear_color_stop(hsla(0., 0., 0., 0.55), 0.),
                                linear_color_stop(hsla(0., 0., 0., 0.), 0.5),
                            )))
                            .child(div().absolute().top_0().left_0().size_full().bg(
                                linear_gradient(
                                    0.,
                                    linear_color_stop(hsla(0., 0., 0., 0.55), 0.),
                                    linear_color_stop(hsla(0., 0., 0., 0.), 0.5),
                                ),
                            ))
                        }
                    })
                    .child(
                        h_flex()
                            .absolute()
                            .bottom_1()
                            .left_1()
                            .gap_1()
                            .px_1p5()
                            .rounded_md()
                            .bg(hsla(0., 0., 0., 0.65))
                            .text_xs()
                            .text_color(white())
                            .child(m.name.clone())
                            .when(m.primary, |d| d.child(icon("star-fill").size_2p5().text_color(hex(0xfbbf24))))
                            .when(changed, |d| d.child(div().size(px(6.)).rounded_full().bg(p.accent))),
                    )
                    .when(dragging, |d| {
                        d.border_color(with_alpha(p.accent, 0.55))
                            .child(div().absolute().top_0().left_0().size_full().bg(with_alpha(p.accent, 0.08)))
                    })
                    .drag_over::<drag::CardDrag>(move |style, _, _, _| {
                        style.border_color(p.accent_hover).bg(with_alpha(p.accent, 0.25))
                    })
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

        div()
            .relative()
            .flex_none()
            .w(px((max_x - min_x) as f32 * scale))
            .h(px((max_y - min_y) as f32 * scale))
            .children(screens)
            .into_any_element()
    }

    /// Monitors whose staged image differs from what's up.
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

    /// The screens stage. `width` is the main column's width.
    pub(super) fn render_stage(&mut self, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let p = pal();
        let shell = || {
            h_flex()
                .id("stage")
                .flex_none()
                .gap_5()
                .px(px(GRID_PAD))
                .py_3()
                .border_t_1()
                .border_color(p.border)
                .bg(with_alpha(p.surface, 0.92))
        };
        if !self.backend.is_supported() {
            return shell()
                .gap_2()
                .text_sm()
                .text_color(p.muted)
                .child(icon("triangle-alert").size_4().text_color(hex(0xf59e0b)))
                .child(format!(
                    "muralist can't set wallpapers on {} yet. You can still browse and download.",
                    self.backend.name()
                ))
                .into_any_element();
        }

        let lock_reach = self.backend.lock().map(|w| w.reach());
        if lock_reach.is_none() {
            self.target = TargetMode::Desktop;
        }
        let mode = self.target;
        let targets = mode.targets();
        let changes = self.pending_changes();
        let busy = self.busy > 0;
        let fetching = self.landing.len();
        let can_spin = !self.results.is_empty() && self.slot.is_none() && !self.monitors.is_empty();
        let has_selection = self.selected.is_some();

        // Status, and what the screens are showing.
        let status: AnyElement = if fetching > 0 {
            h_flex()
                .gap_1p5()
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
                .gap_1p5()
                .text_xs()
                .text_color(p.dim)
                .child(icon("circle-check").size_3().text_color(p.ok))
                .child("up to date")
                .into_any_element()
        };
        let target_seg = |id: &'static str, label: &'static str, m: TargetMode| {
            segment(id, label, mode == m, p).px_2().on_click(cx.listener(move |this, _, _, cx| {
                this.target = m;
                cx.notify();
            }))
        };
        let lock_mode = self.cfg.lock_mode;
        let lock_style = (mode != TargetMode::Desktop && lock_reach == Some(Reach::PerMonitor)).then(|| {
            segmented_track(p)
                .child(
                    segment("lm-per", "Per screen", lock_mode == LockMode::PerMonitor, p)
                        .px_2()
                        .on_click(cx.listener(|this, _, _, cx| this.set_lock_mode(LockMode::PerMonitor, cx))),
                )
                .child(
                    segment("lm-single", "One image", lock_mode == LockMode::Single, p)
                        .px_2()
                        .on_click(cx.listener(|this, _, _, cx| this.set_lock_mode(LockMode::Single, cx))),
                )
        });
        let left = v_flex()
            .flex_none()
            .w(px(STAGE_SIDE_W))
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .child(icon("monitor").size_4().text_color(p.accent))
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Screens"))
                    .child(status),
            )
            .when(lock_reach.is_some(), |d| {
                d.child(
                    segmented_track(p)
                        .child(target_seg("tm-desktop", "Desktop", TargetMode::Desktop))
                        .child(target_seg("tm-lock", "Lock", TargetMode::Lock))
                        .child(target_seg("tm-both", "Both", TargetMode::Both)),
                )
            })
            .children(lock_style)
            .when_some(self.rotation_countdown(), |d, countdown| {
                d.child(
                    h_flex()
                        .gap_1p5()
                        .text_xs()
                        .text_color(p.dim)
                        .child(icon("shuffle").size_3())
                        .child(format!("every {} min  ·  {countdown}", self.cfg.rotate_minutes)),
                )
            });

        // The desk: one layout per target, side by side.
        let center_w = (width - STAGE_SIDE_W * 2. - GRID_PAD * 2. - 40.).max(160.);
        let per_w = (center_w - 24. * (targets.len() as f32 - 1.)) / targets.len() as f32;
        let layouts: Vec<AnyElement> = targets
            .iter()
            .map(|&t| {
                let layout = self.render_layout(t, per_w, LAYOUT_H, cx);
                v_flex()
                    .items_center()
                    .gap_1()
                    .when(targets.len() > 1, |d| d.child(div().text_xs().text_color(p.dim).child(t.label())))
                    .child(layout)
                    .into_any_element()
            })
            .collect();
        let center = h_flex().flex_1().min_w_0().justify_center().items_center().gap_6().children(layouts);

        let mut actions = h_flex()
            .gap_2()
            .child(
                kit::Btn::new("spin")
                    .label("Spin")
                    .icon("dices")
                    .sm()
                    .disabled(!can_spin)
                    .tooltip("Random wallpapers from your results on every screen")
                    .on_click(cx.listener(|this, _, _, cx| this.spin(cx)))
                    .render()
                    .flex_1(),
            )
            .child(
                kit::Btn::new("harmonize")
                    .label("Match")
                    .icon("palette")
                    .sm()
                    .disabled(!can_spin)
                    .tooltip(if has_selection {
                        "Fill the other screens with colors that match the selected wallpaper"
                    } else {
                        "Fill every screen with wallpapers whose colors match"
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.harmonize(cx)))
                    .render()
                    .flex_1(),
            );
        if mode == TargetMode::Lock {
            actions = actions.child(
                kit::Btn::new("copy-desktop")
                    .icon("copy")
                    .sm()
                    .tooltip("Use the desktop's wallpapers on the lock screen")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.lock = this.desktop.clone();
                        cx.notify();
                    })),
            );
        }
        let apply = kit::Btn::new("apply")
            .label(if fetching > 0 {
                format!("Fetching {fetching}")
            } else if changes > 0 {
                format!("Apply {changes}")
            } else {
                "Applied".into()
            })
            .icon("check")
            .primary()
            .full()
            .loading(busy)
            .disabled(changes == 0 && !busy)
            .on_click(cx.listener(|this, _, _, cx| this.apply(this.target.targets(), cx)));
        let right = v_flex().flex_none().w(px(STAGE_SIDE_W)).gap_2().child(actions).child(apply);

        shell()
            .child(left)
            .child(center)
            .child(right)
            .with_animation(
                "stage-in",
                Animation::new(std::time::Duration::from_millis(260)).with_easing(ease_out_quint()),
                |d, t| d.opacity(t),
            )
            .into_any_element()
    }

    /// The inspector's "Use" panel: put the selected wallpaper on the screens.
    pub(super) fn render_put_on(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = pal();
        let Some(item) = self.selected.clone() else {
            return div().into_any_element();
        };
        let supported = self.backend.is_supported() && !self.monitors.is_empty();
        let where_to = match self.target {
            TargetMode::Desktop => "your desktop",
            TargetMode::Lock => "your lock screen",
            TargetMode::Both => "your desktop and lock screen",
        };
        let n = self.monitors.len();

        let set_now = kit::Btn::new("set-now")
            .label("Set as wallpaper")
            .icon("wallpaper")
            .primary()
            .lg()
            .full()
            .disabled(!supported)
            .loading(self.busy > 0)
            .on_click(cx.listener(move |this, _, _, cx| {
                let item = this.selected.clone();
                if let Some(item) = item {
                    this.assign_item_then(item, this.target.targets(), Placement::All, true, cx);
                }
            }));
        let tile = |id: &'static str, icon_name: &'static str, label: &'static str, sub: &'static str| {
            super::kit::surface(v_flex(), p)
                .id(id)
                .flex_1()
                .gap_0p5()
                .p_3()
                .rounded_lg()
                .cursor_pointer()
                .hover(move |s| s.border_color(with_alpha(p.accent, 0.6)))
                .active(|s| s.pt(px(14.)))
                .child(icon(icon_name).size_4().text_color(p.accent))
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(label))
                .child(div().text_xs().text_color(p.dim).child(sub))
        };
        let remote = matches!(item, Item::Remote(_));
        v_flex()
            .gap_3()
            .p(px(CARD_PAD))
            .rounded_xl()
            .bg(with_alpha(p.surface2, 0.82))
            .border_1()
            .border_color(p.border)
            .child(set_now)
            .child(div().text_xs().text_color(p.dim).child(format!("On all {n} screens of {where_to}, straight away.")))
            .when(supported, |d| {
                d.child(
                    h_flex()
                        .gap_2()
                        .child(tile("fill-all", "layout-dashboard", "Stage on all", "Review before applying").on_click(
                            cx.listener(|this, _, _, cx| this.assign(this.target.targets(), Placement::All, cx)),
                        ))
                        .child(
                            tile("span", "frame", "Span", "One image across all")
                                .on_click(cx.listener(|this, _, _, cx| this.open_span_editor(cx))),
                        ),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .text_xs()
                        .text_color(p.dim)
                        .child(icon("mouse-pointer-click").size_3p5())
                        .child("Or click a screen below, or drag the card onto one."),
                )
            })
            .when(remote, |d| {
                d.child(
                    kit::Btn::new("save-original")
                        .label("Save to library")
                        .icon("arrow-down")
                        .ghost()
                        .sm()
                        .on_click(cx.listener(|this, _, _, cx| this.download_selected(cx))),
                )
            })
            .into_any_element()
    }
}
