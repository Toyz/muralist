//! Frame a span: the monitors at true proportions with bezel gaps, the image
//! behind them. Drag to slide it, scroll to zoom; what falls outside the
//! screens is shown dimmed so you can see what gets cut.

use std::path::PathBuf;

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{BtnKind, Item, Muralist, Target, button, icon, icon_button, pal, with_alpha};
use crate::config::cache_dir;
use crate::span::{self, Framing};

/// Room around the desktop where overhanging image shows, dimmed.
const MARGIN: f32 = 44.;
const MAX_ZOOM: f32 = 4.;

pub(super) struct SpanEdit {
    item: Item,
    dims: (u32, u32),
    framing: Framing,
    targets: Vec<Target>,
    drag: Option<Point<Pixels>>,
}

impl Muralist {
    pub(super) fn open_span_editor(&mut self, cx: &mut Context<Self>) {
        let (Some(item), Some(dims)) = (self.selected.clone(), self.selected_dims) else {
            // Unknown size: fall back to a centered span.
            self.assign(self.target.targets(), super::Placement::Span, cx);
            return;
        };
        self.span_edit =
            Some(SpanEdit { item, dims, framing: Framing::default(), targets: self.target.targets(), drag: None });
        cx.notify();
    }

    fn desktop_box(&self) -> (i32, i32, f64, f64) {
        let min_x = self.monitors.iter().map(|m| m.x).min().unwrap_or(0);
        let min_y = self.monitors.iter().map(|m| m.y).min().unwrap_or(0);
        let max_x = self.monitors.iter().map(|m| m.x + m.width as i32).max().unwrap_or(1);
        let max_y = self.monitors.iter().map(|m| m.y + m.height as i32).max().unwrap_or(1);
        (min_x, min_y, f64::from(max_x - min_x), f64::from(max_y - min_y))
    }

    /// Desktop units to on-screen pixels in the editor.
    fn editor_scale(&self) -> f32 {
        let (_, _, bw, bh) = self.desktop_box();
        let avail_w = (self.viewport.0 - 220.).max(300.);
        let avail_h = (self.viewport.1 - 330.).max(160.);
        (avail_w / bw as f32).min(avail_h / bh as f32)
    }

    fn span_drag(&mut self, pos: Point<Pixels>, cx: &mut Context<Self>) {
        let k = f64::from(self.editor_scale());
        let (_, _, bw, bh) = self.desktop_box();
        let Some(edit) = &mut self.span_edit else { return };
        let Some(last) = edit.drag else { return };
        let (s, off_x, off_y) = span::layout(edit.dims.0, edit.dims.1, bw, bh, edit.framing);
        let slack_x = (f64::from(edit.dims.0) * s - bw).max(0.);
        let slack_y = (f64::from(edit.dims.1) * s - bh).max(0.);
        // Dragging right reveals more of the left: overhang shrinks.
        let new_x = (off_x - f64::from(f32::from(pos.x - last.x)) / k).clamp(0., slack_x);
        let new_y = (off_y - f64::from(f32::from(pos.y - last.y)) / k).clamp(0., slack_y);
        let to_pan = |off: f64, slack: f64| if slack > 0. { (off / (slack / 2.) - 1.) as f32 } else { 0. };
        edit.framing.pan = (to_pan(new_x, slack_x), to_pan(new_y, slack_y));
        edit.drag = Some(pos);
        cx.notify();
    }

    fn commit_span(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.span_edit.take() else { return };
        let client = self.client.clone();
        let dir = self.cfg.download_dir.clone();
        let monitors = self.monitors.clone();
        let framing = edit.framing;
        let targets = edit.targets;
        self.busy += 1;
        self.set_status("Slicing across displays...", false, cx);
        let item = edit.item;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let path = Muralist::fetch(&client, &dir, item)?;
                    span::slice_framed(&path, &monitors, &cache_dir().join("span"), framing)
                })
                .await;
            this.update(cx, |this, cx| {
                this.busy -= 1;
                match result {
                    Ok(map) => {
                        for t in &targets {
                            match t {
                                Target::Desktop => this.desktop.extend(map.clone()),
                                Target::Lock => this.lock.extend(map.clone()),
                            }
                        }
                        this.set_status("Span staged. Press Apply to set it.", false, cx);
                    }
                    Err(e) => this.set_status(format!("Span failed: {e:#}"), true, cx),
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Sharpest copy of the image already on hand for the editor.
    fn editor_image(&mut self, item: &Item, cx: &mut Context<Self>) -> Option<PathBuf> {
        let full = match item {
            Item::Remote(w) => self.remote_full(w, cx),
            Item::Local(path) => Some(path.clone()),
        };
        if let Some(full) = full {
            let bucket = |v: f32| ((v * self.scale / 256.).ceil() * 256.) as u32;
            let (lw, lh) = (bucket(self.viewport.0 - 168.), bucket(self.viewport.1 - 112.));
            if let Some(fit) = self.fitted(&full, lw, lh) {
                return Some(fit);
            }
            let pw = self.fit_width();
            let ph = (self.preview_h() * self.scale).round() as u32;
            if let Some(fit) = self.fitted(&full, pw, ph) {
                return Some(fit);
            }
        }
        match item {
            Item::Remote(w) => self.remote_thumb(w, true),
            Item::Local(path) => self.local_thumb(path),
        }
    }

    pub(super) fn render_span_editor(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = pal();
        let (item, dims, framing) = {
            let edit = self.span_edit.as_ref()?;
            (edit.item.clone(), edit.dims, edit.framing)
        };
        let image = self.editor_image(&item, cx);
        let (min_x, min_y, bw, bh) = self.desktop_box();
        let k = self.editor_scale();
        let (s, off_x, off_y) = span::layout(dims.0, dims.1, bw, bh, framing);
        let img_w = (f64::from(dims.0) * s) as f32 * k;
        let img_h = (f64::from(dims.1) * s) as f32 * k;
        let img_left = MARGIN - off_x as f32 * k;
        let img_top = MARGIN - off_y as f32 * k;
        let stage_w = bw as f32 * k + MARGIN * 2.;
        let stage_h = bh as f32 * k + MARGIN * 2.;
        let gap = 6.;

        let picture = |path: PathBuf, left: f32, top: f32| {
            img(path).absolute().left(px(left)).top(px(top)).w(px(img_w)).h(px(img_h)).object_fit(ObjectFit::Fill)
        };

        // Each monitor is a window onto the same image at full brightness.
        let windows: Vec<AnyElement> = self
            .monitors
            .clone()
            .into_iter()
            .map(|m| {
                let left = MARGIN + (m.x - min_x) as f32 * k + gap / 2.;
                let top = MARGIN + (m.y - min_y) as f32 * k + gap / 2.;
                div()
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .w(px(m.width as f32 * k - gap))
                    .h(px(m.height as f32 * k - gap))
                    .overflow_hidden()
                    .border_1()
                    .border_color(hsla(0., 0., 1., 0.35))
                    .when_some(image.clone(), |d, path| d.child(picture(path, img_left - left, img_top - top)))
                    .child(
                        div()
                            .absolute()
                            .top_1()
                            .left_1()
                            .px_1p5()
                            .rounded_md()
                            .bg(hsla(0., 0., 0., 0.6))
                            .text_xs()
                            .text_color(white())
                            .child(m.name),
                    )
                    .into_any_element()
            })
            .collect();

        let stage = div()
            .id("span-stage")
            .relative()
            .w(px(stage_w))
            .h(px(stage_h))
            .overflow_hidden()
            .rounded_xl()
            .bg(hsla(0., 0., 0., 0.5))
            .cursor(if self.span_edit.as_ref().is_some_and(|e| e.drag.is_some()) {
                CursorStyle::ClosedHand
            } else {
                CursorStyle::OpenHand
            })
            // The whole image, dimmed: everything outside the screens gets cut.
            .when_some(image, |d, path| d.child(picture(path, img_left, img_top).opacity(0.22)))
            .children(windows)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                    if ev.click_count >= 2 {
                        if let Some(edit) = &mut this.span_edit {
                            edit.framing = Framing::default();
                        }
                        cx.notify();
                        return;
                    }
                    if let Some(edit) = &mut this.span_edit {
                        edit.drag = Some(ev.position);
                    }
                }),
            )
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                if ev.pressed_button == Some(MouseButton::Left) {
                    this.span_drag(ev.position, cx);
                } else if let Some(edit) = &mut this.span_edit {
                    edit.drag = None;
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if let Some(edit) = &mut this.span_edit {
                        edit.drag = None;
                    }
                    cx.notify();
                }),
            )
            .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, _, cx| {
                let dy = f32::from(ev.delta.pixel_delta(px(20.)).y);
                if let Some(edit) = &mut this.span_edit {
                    edit.framing.zoom = (edit.framing.zoom * (1. + dy * 0.004)).clamp(1., MAX_ZOOM);
                }
                cx.notify();
            }));

        let card = v_flex()
            .id("span-card")
            .rounded_xl()
            .bg(p.surface)
            .border_1()
            .border_color(p.border_strong)
            .shadow_lg()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                h_flex()
                    .gap_2p5()
                    .px_6()
                    .h(px(56.))
                    .border_b_1()
                    .border_color(p.border)
                    .child(icon("frame").size_4().text_color(p.accent))
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Frame the span"))
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .text_color(p.dim)
                            .child("Drag to move  ·  scroll to zoom  ·  double-click to reset"),
                    )
                    .child(icon_button("span-close", "close", p).on_click(cx.listener(|this, _, _, cx| {
                        this.span_edit = None;
                        cx.notify();
                    }))),
            )
            .child(div().p_6().child(stage))
            .child(
                h_flex()
                    .justify_between()
                    .px_6()
                    .h(px(64.))
                    .border_t_1()
                    .border_color(p.border)
                    .child(div().text_xs().text_color(p.dim).child(format!("Zoom {:.0}%", framing.zoom * 100.)))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(button("span-cancel", "Cancel", BtnKind::Secondary, p).on_click(cx.listener(
                                |this, _, _, cx| {
                                    this.span_edit = None;
                                    cx.notify();
                                },
                            )))
                            .child(
                                button("span-commit", "Span it", BtnKind::Primary, p)
                                    .on_click(cx.listener(|this, _, _, cx| this.commit_span(cx))),
                            ),
                    ),
            );

        Some(
            div()
                .id("span-overlay")
                .occlude()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(with_alpha(p.bg, 0.8))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.span_edit = None;
                        cx.notify();
                    }),
                )
                .child(card)
                .into_any_element(),
        )
    }
}
