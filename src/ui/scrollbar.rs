//! The grid's scrollbar: a slim rail down its right edge with a thumb to
//! drag (or click the rail to jump there), widening under the pointer.
//! Dragging moves by how far the pointer travels, as a scrollbar does, and
//! the next page waits until the drag ends, so a page landing mid-drag
//! can't pull the thumb away and fetch another.

use gpui_kit::*;

use super::{GRID_PAD, Muralist, pal, with_alpha};

/// A drag in progress: where the pointer and the grid were when it began,
/// and how far the grid moves per pixel of pointer travel.
#[derive(Clone, Copy)]
pub(super) struct ScrollDrag {
    y0: f32,
    off0: f32,
    per_px: f32,
}

const RAIL_W: f32 = 8.;
const RAIL_HOVER_W: f32 = 12.;
const MIN_THUMB: f32 = 28.;

impl Muralist {
    /// The scrolled distance, the most it can scroll and the visible
    /// height, from the grid's scroll handle.
    fn grid_scroll_metrics(&self) -> (f32, f32, f32) {
        let state = self.grid_scroll.0.borrow();
        let handle = &state.base_handle;
        let off = -f32::from(handle.offset().y);
        let max = f32::from(handle.max_offset().y).max(0.);
        let view = f32::from(handle.bounds().size.height);
        (off.clamp(0., max), max, view)
    }

    fn set_grid_offset(&mut self, off: f32, cx: &mut Context<Self>) {
        let (_, max, _) = self.grid_scroll_metrics();
        let state = self.grid_scroll.0.borrow();
        let x = state.base_handle.offset().x;
        state.base_handle.set_offset(point(x, px(-off.clamp(0., max))));
        cx.notify();
    }

    /// Follows a drag from the root, so it keeps going off the rail.
    pub(super) fn drag_scrollbar(&mut self, y: Pixels, cx: &mut Context<Self>) {
        let Some(d) = self.grid_drag else { return };
        self.set_grid_offset(d.off0 + (f32::from(y) - d.y0) * d.per_px, cx);
    }

    pub(super) fn render_scrollbar(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = pal();
        let (off, max, view) = self.grid_scroll_metrics();
        if max <= 1. || view <= 0. {
            return None;
        }
        let rail_h = view;
        let thumb_h = (view / (view + max) * rail_h).max(MIN_THUMB).min(rail_h);
        let travel = (rail_h - thumb_h).max(1.);
        let thumb_top = off / max * travel;
        // Plain: no image colors, no accent; just brighter when it's in use.
        let (thumb, thumb_on) = (with_alpha(p.muted, 0.45), with_alpha(p.fg, 0.7));
        let dragging = self.grid_drag.is_some();
        Some(
            div()
                .id("grid-scrollbar")
                .group("grid-scrollbar")
                .absolute()
                .top(px(GRID_PAD))
                .right(px(3.))
                .h(px(rail_h))
                .w(px(if dragging { RAIL_HOVER_W } else { RAIL_W }))
                .hover(|s| s.w(px(RAIL_HOVER_W)))
                .rounded_full()
                .bg(with_alpha(p.bg, 0.5))
                .border_1()
                .border_color(p.border)
                .cursor_pointer()
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .top(px(thumb_top))
                        .h(px(thumb_h))
                        .rounded_full()
                        .bg(if dragging { thumb_on } else { thumb })
                        .group_hover("grid-scrollbar", move |s| s.bg(thumb_on)),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                        let top = f32::from(this.grid_scroll.0.borrow().base_handle.bounds().origin.y);
                        let y = f32::from(ev.position.y) - top;
                        // On the thumb: drag from where it is. Elsewhere: go
                        // there first, the thumb centred on the click.
                        if !(thumb_top..thumb_top + thumb_h).contains(&y) {
                            this.set_grid_offset((y - thumb_h / 2.) / travel * max, cx);
                        }
                        let (off0, _, _) = this.grid_scroll_metrics();
                        this.grid_drag = Some(ScrollDrag { y0: f32::from(ev.position.y), off0, per_px: max / travel });
                        cx.stop_propagation();
                    }),
                )
                .into_any_element(),
        )
    }
}
