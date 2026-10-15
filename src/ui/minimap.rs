//! The grid's scrollbar is a minimap: a rail down its right edge where
//! every row of results is a strip of its wallpapers' own colors, with a
//! glass thumb over the part on screen. Drag the thumb, or click anywhere
//! on the rail to jump there.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use gpui_kit::prelude::FluentBuilder as _;

use super::{GRID_PAD, GridMode, Item, Muralist, boxed, gridmodes, pal, with_alpha};

/// A thumb drag in progress: where on the thumb it was grabbed, and the
/// rail and content heights it started with.
#[derive(Clone, Copy)]
pub(super) struct MinimapDrag {
    grab: f32,
    rail: f32,
    content: f32,
}

const RAIL_W: f32 = 8.;
/// Opened under the pointer, it's a minimap this wide.
const PANEL_W: f32 = 132.;
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
        let accent = p.accent;
        let rail_h = view;
        let dragging = self.grid_drag.is_some();
        let open = self.minimap_open || dragging;

        // Open, it's a picture of the whole grid: thumbnails while rows are
        // tall enough to show them, color strips once they aren't.
        let panel = open.then(|| {
            let per_strip = rows.len().div_ceil(MAX_STRIPS).max(1);
            let strip_h = rail_h / rows.len().div_ceil(per_strip).max(1) as f32;
            let pictures = strip_h >= 7.;
            let unknown = with_alpha(p.border_strong, 0.6);
            let strips: Vec<AnyElement> = rows
                .chunks(per_strip)
                .map(|chunk| {
                    let row = &chunk[0];
                    let cw = ((PANEL_W - 8.) - (row.len().saturating_sub(1)) as f32) / row.len().max(1) as f32;
                    h_flex()
                        .flex_none()
                        .h(px(strip_h))
                        .gap(px(1.))
                        .pb(px(if pictures { 1. } else { 0. }))
                        .children(row.iter().map(|&ix| {
                            let item = items.get(ix);
                            let color = item
                                .and_then(|i| gridmodes::first_color(i, &self.local_palettes))
                                .map_or(unknown, |c| Hsla { a: 0.9, ..c });
                            let thumb = item.filter(|_| pictures).and_then(|i| match i {
                                Item::Remote(w) => self.remote_thumb(w, false),
                                Item::Local(path) => self.local_thumb(path),
                            });
                            div()
                                .flex_none()
                                .w(px(cw))
                                .h_full()
                                .rounded(px(if pictures { 2. } else { 0. }))
                                .overflow_hidden()
                                .bg(color)
                                .when_some(thumb, |d, t| {
                                    d.child(boxed(img(t), cw, strip_h - 1.).object_fit(ObjectFit::Cover).opacity(0.85))
                                })
                        }))
                        .into_any_element()
                })
                .collect();
            // Which results are on screen, as a readout by the window.
            let n_rows = rows.len().max(1) as f32;
            let first_row = ((off / content) * n_rows).floor() as usize;
            let last_row = (((off + view) / content) * n_rows).ceil() as usize;
            let first = rows.get(first_row).and_then(|r| r.first()).map_or(0, |ix| ix + 1);
            let last =
                rows.get(last_row.saturating_sub(1).min(rows.len() - 1)).and_then(|r| r.last()).map_or(0, |ix| ix + 1);
            let more = if self.page < self.last_page && self.tab == super::Tab::Discover { "+" } else { "" };
            v_flex().size_full().p(px(3.)).children(strips).child(
                div()
                    .absolute()
                    .right(px(PANEL_W + 6.))
                    .top(px((thumb_top + thumb_h / 2. - 10.).clamp(0., rail_h - 20.)))
                    .px_1p5()
                    .py_0p5()
                    .rounded_md()
                    .bg(hsla(0., 0., 0., 0.75))
                    .border_1()
                    .border_color(with_alpha(accent, 0.6))
                    .text_size(px(10.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(white())
                    .whitespace_nowrap()
                    .child(format!("{first}-{last} of {}{more}", items.len())),
            )
        });

        let width = if open { PANEL_W } else { RAIL_W };
        Some(
            div()
                .id("grid-minimap")
                .absolute()
                .top(px(GRID_PAD))
                .right(px(3.))
                .h(px(rail_h))
                .w(px(width))
                .rounded_md()
                .bg(if open { hsla(0., 0., 0., 0.72) } else { with_alpha(p.bg, 0.5) })
                .border_1()
                .border_color(if open { with_alpha(accent, 0.4) } else { p.border })
                .when(open, |d| d.shadow_lg())
                .cursor_pointer()
                .children(panel)
                // What's on screen: a glass window, the scrollbar's thumb.
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .top(px(thumb_top))
                        .h(px(thumb_h))
                        .rounded_sm()
                        .bg(if open { hsla(0., 0., 1., 0.1) } else { with_alpha(accent, 0.55) })
                        .border_1()
                        .border_color(accent)
                        .shadow(super::glass::ambilight(accent, 10., 0.7)),
                )
                .on_hover(cx.listener(|this, over: &bool, _, cx| {
                    if this.minimap_open != *over {
                        this.minimap_open = *over;
                        cx.notify();
                    }
                }))
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
                .with_animation(
                    SharedString::from(format!("minimap-{open}")),
                    Animation::new(std::time::Duration::from_millis(170)).with_easing(ease_out_quint()),
                    move |d, t| {
                        let (from, to) = if open { (RAIL_W, PANEL_W) } else { (PANEL_W, RAIL_W) };
                        d.w(px(from + (to - from) * t))
                    },
                )
                .into_any_element(),
        )
    }
}
