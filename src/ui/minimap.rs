//! The grid's scrollbar is a minimap: a rail down its right edge where
//! every row of results is a strip of its wallpapers' own colors, with a
//! glass thumb over the part on screen. Drag the thumb, or click anywhere
//! on the rail to jump there.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use super::{GRID_PAD, GridMode, Item, Muralist, gridmodes, pal, with_alpha};

/// A thumb drag in progress: where on the thumb it was grabbed, and the
/// rail and content heights it started with.
#[derive(Clone, Copy)]
pub(super) struct MinimapDrag {
    grab: f32,
    rail: f32,
    content: f32,
}

const RAIL_W: f32 = 9.;
const RAIL_HOVER_W: f32 = 16.;
const MIN_THUMB: f32 = 28.;
/// More rows than this are drawn a few to a strip.
const MAX_STRIPS: usize = 220;

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

    /// Scrolls so the thumb's top sits at `thumb_top` along a rail `rail_h`
    /// tall, measured against `content` (frozen for a drag, so pages that
    /// land meanwhile don't yank the thumb out from under the pointer).
    fn scroll_grid_to(&mut self, thumb_top: f32, rail_h: f32, content: f32, cx: &mut Context<Self>) {
        let (_, max, _) = self.grid_scroll_metrics();
        let off = (thumb_top / rail_h.max(1.) * content.max(1.)).clamp(0., max);
        let state = self.grid_scroll.0.borrow();
        let x = state.base_handle.offset().x;
        state.base_handle.set_offset(point(x, px(-off)));
        cx.notify();
    }

    /// Follows a thumb drag from the root, so it keeps going off the rail.
    pub(super) fn drag_minimap(&mut self, y: Pixels, cx: &mut Context<Self>) {
        let Some(drag) = self.grid_drag else { return };
        let top = f32::from(self.grid_scroll.0.borrow().base_handle.bounds().origin.y);
        self.scroll_grid_to(f32::from(y) - top - drag.grab, drag.rail, drag.content, cx);
    }

    pub(super) fn render_minimap(&mut self, items: &[Item], cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = pal();
        let (off, max, view) = self.grid_scroll_metrics();
        if max <= 1. || view <= 0. {
            return None;
        }
        // Mid-drag the scale stays as it was when the drag began.
        let content = self.grid_drag.map_or(view + max, |d| d.content);
        let thumb_h = (view / content * view).max(MIN_THUMB).min(view);
        let thumb_top = (off / content * view).min(view - thumb_h);

        // The rows as the grid lays them out.
        let rows: Vec<Vec<usize>> = if self.cfg.layout == GridMode::Mosaic {
            self.justified.iter().map(|row| row.iter().map(|(ix, _)| *ix).collect()).collect()
        } else {
            let cols = self.grid_layout.cols.max(1);
            (0..items.len()).collect::<Vec<_>>().chunks(cols).map(<[usize]>::to_vec).collect()
        };
        let per_strip = rows.len().div_ceil(MAX_STRIPS).max(1);
        let unknown = with_alpha(p.border_strong, 0.6);
        let strips = rows.chunks(per_strip).map(|chunk| {
            let row = &chunk[0];
            h_flex().flex_1().min_h(px(1.)).children(row.iter().map(|&ix| {
                let color = items
                    .get(ix)
                    .and_then(|i| gridmodes::first_color(i, &self.local_palettes))
                    .map_or(unknown, |c| Hsla { a: 0.85, ..c });
                div().flex_1().h_full().bg(color)
            }))
        });
        let accent = p.accent;
        let rail_h = view;
        let dragging = self.grid_drag.is_some();
        Some(
            div()
                .id("grid-minimap")
                .absolute()
                .top(px(GRID_PAD))
                .right(px(3.))
                .h(px(rail_h))
                .w(px(if dragging { RAIL_HOVER_W } else { RAIL_W }))
                .hover(|s| s.w(px(RAIL_HOVER_W)))
                .rounded_md()
                .overflow_hidden()
                .bg(with_alpha(p.bg, 0.7))
                .border_1()
                .border_color(p.border)
                .cursor_pointer()
                .child(v_flex().size_full().opacity(0.9).children(strips))
                // What's on screen: a glass window over the strips.
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .top(px(thumb_top))
                        .h(px(thumb_h))
                        .rounded_sm()
                        .bg(hsla(0., 0., 1., 0.18))
                        .border_1()
                        .border_color(accent)
                        .shadow(super::glass::ambilight(accent, 10., 0.7)),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                        let top = f32::from(this.grid_scroll.0.borrow().base_handle.bounds().origin.y);
                        let y = f32::from(ev.position.y) - top;
                        // On the thumb: grab it where it was clicked. Elsewhere:
                        // jump there, grabbing the thumb by its middle.
                        let grab =
                            if (thumb_top..thumb_top + thumb_h).contains(&y) { y - thumb_top } else { thumb_h / 2. };
                        this.grid_drag = Some(MinimapDrag { grab, rail: rail_h, content });
                        this.scroll_grid_to(y - grab, rail_h, content, cx);
                        cx.stop_propagation();
                    }),
                )
                .into_any_element(),
        )
    }
}
