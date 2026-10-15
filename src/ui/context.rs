//! The right-click card: the wallpaper up top in its own colors, your
//! screens to put it on, then its actions as rows with shortcut keys.
//! Galleries open inline. Click outside or press Esc to close.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::galleries::entry_of;
use super::{
    Item, Muralist, Placement, box_ratio, boxed, file_name, glass, gridmodes, icon, pal, purity_color, purity_label,
    radius, with_alpha,
};
use crate::gallery::Source;

/// How much the card grows, and the bounds on its lifted size.
const LIFT: f32 = 1.6;
const MIN_W: f32 = 380.;
const MAX_W: f32 = 620.;
const MIN_H: f32 = 330.;
/// Corner radius of the lifted card.
const R: f32 = 14.;
/// Room for the screens along the top.
const DESK_W: f32 = 150.;
const DESK_H: f32 = 34.;

pub(super) struct ContextMenu {
    pub item: Item,
    pub at: Point<Pixels>,
    /// Where the card sat in the grid, to lift it from (and back to).
    pub origin: Option<Bounds<Pixels>>,
    /// Unique per opening, so a close can't remove a newer card.
    pub id: u64,
    /// Settling back into the grid; removed when that finishes.
    pub closing: bool,
}

/// How long the card takes to lift, and to settle back.
const OPEN_MS: u64 = 220;
const CLOSE_MS: u64 = 180;

#[derive(Clone)]
enum Action {
    SetAll,
    SetOn(String),
    Span,
    Galleries,
    Similar(String),
    Download,
    Open,
}

impl Muralist {
    pub(super) fn open_context(
        &mut self,
        item: Item,
        at: Point<Pixels>,
        origin: Option<Bounds<Pixels>>,
        cx: &mut Context<Self>,
    ) {
        self.picker = None;
        self.context_hover = None;
        self.context_galleries = false;
        self.context_seq += 1;
        self.context = Some(ContextMenu { item, at, origin, id: self.context_seq, closing: false });
        cx.notify();
    }

    /// Settles the card back into its spot, then removes it.
    pub(super) fn close_context(&mut self, cx: &mut Context<Self>) {
        let Some(menu) = self.context.as_mut().filter(|m| !m.closing) else { return };
        menu.closing = true;
        let id = menu.id;
        self.context_hover = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(CLOSE_MS)).await;
            this.update(cx, |this, cx| {
                if this.context.as_ref().is_some_and(|m| m.id == id) {
                    this.context = None;
                    this.context_galleries = false;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn run_context(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(action, Action::Galleries) {
            self.context_galleries = !self.context_galleries;
            cx.notify();
            return;
        }
        let Some(item) = self.context.as_ref().filter(|m| !m.closing).map(|m| m.item.clone()) else { return };
        self.close_context(cx);
        let targets = self.target.targets();
        match action {
            Action::SetAll => self.assign_item_then(item, targets, Placement::All, true, cx),
            Action::SetOn(name) => self.assign_item_then(item, targets, Placement::Monitor(name), true, cx),
            Action::Span => {
                self.select(item, window, cx);
                self.open_span_editor(cx);
            }
            Action::Similar(id) => self.search_for(format!("like:{id}"), window, cx),
            Action::Download => {
                self.select(item, window, cx);
                self.download_selected(cx);
            }
            Action::Open => {
                self.select(item, window, cx);
                self.open_lightbox(window, cx);
            }
            Action::Galleries => {}
        }
        cx.notify();
    }

    /// Shortcut keys while the card is open. Returns whether it was used.
    pub(super) fn context_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(menu) = self.context.as_ref().filter(|m| !m.closing) else { return false };
        let remote_id = match &menu.item {
            Item::Remote(w) => Some(w.id.clone()),
            Item::Local(_) => None,
        };
        let can_set = self.backend.is_supported() && !self.monitors.is_empty();
        let action = match key {
            "s" if can_set => Action::SetAll,
            "p" if can_set && self.monitors.len() > 1 => Action::Span,
            "g" => Action::Galleries,
            "f" => match remote_id {
                Some(id) => Action::Similar(id),
                None => return false,
            },
            "d" if remote_id.is_some() => Action::Download,
            "enter" => Action::Open,
            k if can_set && k.len() == 1 && k.as_bytes()[0].is_ascii_digit() => {
                let n = (k.as_bytes()[0] - b'0') as usize;
                match self.monitors.get(n.wrapping_sub(1)) {
                    Some(m) => Action::SetOn(m.name.clone()),
                    None => return false,
                }
            }
            _ => return false,
        };
        self.run_context(action, window, cx);
        true
    }

    pub(super) fn render_context(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = pal();
        let menu = self.context.as_ref()?;
        let (item, at, origin, open_id, closing) = (menu.item.clone(), menu.at, menu.origin, menu.id, menu.closing);
        let colors = gridmodes::item_colors(&item, &self.local_palettes);
        let c = glass::glow_color(&colors).unwrap_or(p.accent);
        let thumb = match &item {
            Item::Remote(w) => self.remote_thumb(w, true).or_else(|| self.remote_thumb(w, false)),
            Item::Local(path) => self.local_thumb(path),
        };
        let can_set = self.backend.is_supported() && !self.monitors.is_empty();
        let key = match &item {
            Item::Remote(w) => w.id.clone(),
            Item::Local(path) => file_name(path),
        };

        // Where the card was, and where it grows to: about 1.6x around its
        // own center, tall enough for the actions, inside the window.
        let (vw, vh) = self.viewport;
        let from =
            origin.unwrap_or_else(|| Bounds::new(point(at.x - px(140.), at.y - px(88.)), size(px(280.), px(175.))));
        let (fx, fy, fw, fh) = (
            f32::from(from.origin.x),
            f32::from(from.origin.y),
            f32::from(from.size.width),
            f32::from(from.size.height),
        );
        let ratio = fw / fh.max(1.);
        let mut w = (fw * LIFT).clamp(MIN_W, MAX_W);
        let mut h = w / ratio;
        if h < MIN_H {
            h = MIN_H;
            w = (h * ratio).min(vw - 32.);
        }
        if h > vh - 32. {
            h = vh - 32.;
            w = (h * ratio).min(vw - 32.);
        }
        let (cx_, cy_) = (fx + fw / 2., fy + fh / 2.);
        let tx = (cx_ - w / 2.).clamp(16., (vw - w - 16.).max(16.));
        let ty = (cy_ - h / 2.).clamp(16., (vh - h - 16.).max(16.));

        // ---- the screens, along the top of the image ----
        let desk = can_set.then(|| {
            let min_x = self.monitors.iter().map(|m| m.x).min().unwrap_or(0);
            let min_y = self.monitors.iter().map(|m| m.y).min().unwrap_or(0);
            let max_x = self.monitors.iter().map(|m| m.x + m.width as i32).max().unwrap_or(1);
            let max_y = self.monitors.iter().map(|m| m.y + m.height as i32).max().unwrap_or(1);
            let scale = (DESK_W / (max_x - min_x).max(1) as f32).min(DESK_H / (max_y - min_y).max(1) as f32);
            let gap = 4.;
            let screens =
                self.monitors.clone().into_iter().enumerate().map(|(i, m)| {
                    let (sw, sh) = (m.width as f32 * scale - gap, m.height as f32 * scale - gap);
                    let hovered = self.context_hover == Some(i);
                    let name = m.name.clone();
                    div()
                        .id(("ctx-screen", i))
                        .absolute()
                        .left(px((m.x - min_x) as f32 * scale + gap / 2.))
                        .top(px((m.y - min_y) as f32 * scale + gap / 2.))
                        .w(px(sw))
                        .h(px(sh))
                        .map(|d| radius(d, 5.))
                        .bg(hsla(0., 0., 0., 0.55))
                        .cursor_pointer()
                        // Hover ghosts the wallpaper onto that screen.
                        .when_some(thumb.clone().filter(|_| hovered), |d, t| {
                            d.child(radius(boxed(img(t), sw, sh).object_fit(ObjectFit::Cover), 5.))
                        })
                        .child(
                            div()
                                .absolute()
                                .inset_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_size(px(10.))
                                .text_color(white())
                                .child(format!("{}", i + 1)),
                        )
                        .child(div().absolute().inset_0().map(|d| radius(d, 5.)).border_1().border_color(if hovered {
                            c
                        } else {
                            hsla(0., 0., 1., 0.45)
                        }))
                        .tooltip(move |window, cx| {
                            gpui_kit::component::tooltip::Tooltip::new(format!("Set on {} ({})", m.name, i + 1))
                                .build(window, cx)
                        })
                        .on_hover(cx.listener(move |this, over: &bool, _, cx| {
                            let next = if *over { Some(i) } else { this.context_hover.filter(|h| *h != i) };
                            if this.context_hover != next {
                                this.context_hover = next;
                                cx.notify();
                            }
                        }))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.run_context(Action::SetOn(name.clone()), window, cx)
                        }))
                });
            h_flex()
                .gap_2()
                .p_1p5()
                .rounded_lg()
                .bg(hsla(0., 0., 0., 0.45))
                .border_1()
                .border_color(hsla(0., 0., 1., 0.12))
                .child(
                    div()
                        .relative()
                        .flex_none()
                        .w(px((max_x - min_x) as f32 * scale))
                        .h(px((max_y - min_y) as f32 * scale))
                        .children(screens),
                )
                .child(
                    div()
                        .id("ctx-all")
                        .px_2()
                        .h(px((max_y - min_y) as f32 * scale - gap))
                        .flex()
                        .items_center()
                        .rounded_md()
                        .border_1()
                        .border_color(hsla(0., 0., 1., 0.45))
                        .text_xs()
                        .text_color(white())
                        .cursor_pointer()
                        .hover(move |s| s.border_color(c).bg(with_alpha(c, 0.35)))
                        .child("All")
                        .on_click(cx.listener(|this, _, window, cx| this.run_context(Action::SetAll, window, cx))),
                )
        });

        // ---- the actions, on frosted glass over the image ----
        let galleries_open = self.context_galleries;
        let gallery_list = galleries_open.then(|| self.render_context_galleries(&item, c, cx));
        let button =
            |id: &'static str, icon_name: &'static str, label: &'static str, hint: &'static str, action: Action| {
                v_flex()
                    .id(id)
                    .relative()
                    .items_center()
                    .justify_center()
                    .gap_1()
                    .h(px(54.))
                    .rounded_lg()
                    .bg(hsla(0., 0., 1., 0.07))
                    .border_1()
                    .border_color(hsla(0., 0., 1., 0.1))
                    .text_xs()
                    .text_color(hsla(0., 0., 1., 0.9))
                    .cursor_pointer()
                    .hover(move |s| s.bg(with_alpha(c, 0.32)).border_color(with_alpha(c, 0.8)))
                    .child(icon(icon_name).size_4())
                    .child(label)
                    .child(
                        div()
                            .absolute()
                            .top(px(4.))
                            .right(px(6.))
                            .text_size(px(9.))
                            .text_color(hsla(0., 0., 1., 0.45))
                            .child(hint),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| this.run_context(action.clone(), window, cx)))
                    .into_any_element()
            };
        let remote_id = match &item {
            Item::Remote(w) => Some(w.id.clone()),
            Item::Local(_) => None,
        };
        let mut buttons: Vec<AnyElement> = Vec::new();
        if can_set {
            buttons.push(button("ctx-set", "wallpaper", "Set", "S", Action::SetAll));
            if self.monitors.len() > 1 {
                buttons.push(button("ctx-span", "frame", "Span", "P", Action::Span));
            }
        }
        buttons.push(button("ctx-galleries", "bookmark", "Gallery", "G", Action::Galleries));
        if let Some(id) = remote_id {
            buttons.push(button("ctx-similar", "search", "Similar", "F", Action::Similar(id)));
            buttons.push(button("ctx-download", "arrow-down", "Save", "D", Action::Download));
        }
        buttons.push(button("ctx-open", "maximize-2", "Open", "Enter", Action::Open));
        let cols = if buttons.len() > 4 { 3 } else { buttons.len() as u16 };

        let (title, chips): (String, Vec<AnyElement>) = match &item {
            Item::Remote(w) => {
                let pc = purity_color(&w.purity);
                (
                    w.id.clone(),
                    vec![
                        h_flex()
                            .gap_1()
                            .px_1p5()
                            .rounded_full()
                            .bg(with_alpha(pc, 0.3))
                            .text_color(pc)
                            .child(div().size(px(5.)).rounded_full().bg(pc))
                            .child(purity_label(&w.purity))
                            .into_any_element(),
                        div().child(w.resolution.clone()).into_any_element(),
                    ],
                )
            }
            Item::Local(path) => (file_name(path), Vec::new()),
        };
        let glass = v_flex()
            .gap_2p5()
            .p_3()
            .map(|d| super::radius_bottom(d, R))
            .bg(linear_gradient(
                180.,
                linear_color_stop(Hsla { l: 0.1, s: c.s.min(0.55), a: 0.72, ..c }, 0.),
                linear_color_stop(Hsla { l: 0.06, s: c.s.min(0.4), a: 0.94, ..c }, 1.),
            ))
            .border_t_1()
            .border_color(with_alpha(c, 0.45))
            .child(
                h_flex()
                    .gap_2()
                    .text_xs()
                    .text_color(hsla(0., 0., 1., 0.75))
                    .child(
                        div().text_sm().font_weight(FontWeight::SEMIBOLD).text_color(white()).truncate().child(title),
                    )
                    .children(chips),
            )
            .map(|d| match gallery_list {
                Some(list) => d.child(
                    v_flex()
                        .gap_1()
                        .child(
                            h_flex()
                                .id("ctx-galleries-back")
                                .gap_1p5()
                                .text_xs()
                                .text_color(hsla(0., 0., 1., 0.6))
                                .cursor_pointer()
                                .hover(|s| s.text_color(white()))
                                .child(icon("chevron-left").size_3p5())
                                .child("Save to gallery")
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.run_context(Action::Galleries, window, cx)),
                                ),
                        )
                        .child(list),
                ),
                None => d.child(div().grid().grid_cols(cols).gap_1p5().children(buttons)),
            });

        let card = v_flex()
            .id("context-card")
            .absolute()
            .rounded(px(R))
            .bg(p.bg)
            .shadow_lg()
            .occlude()
            // The picture itself, edge to edge.
            .child(div().absolute().inset_0().when_some(thumb, |d, t| {
                d.child(radius(box_ratio(img(t).size_full(), w, h).object_fit(ObjectFit::Cover), R))
            }))
            .child(h_flex().relative().p_2p5().children(desk))
            .child(div().flex_1())
            .child(div().relative().child(glass))
            // Its edge in its own color, on top.
            .child(div().absolute().inset_0().rounded(px(R)).border_1().border_color(with_alpha(c, 0.7)))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            // Grow out of the card's own spot in the grid, and settle back
            // into it on close.
            .with_animation(
                SharedString::from(format!("context-{}-{key}-{open_id}", if closing { "settle" } else { "lift" })),
                Animation::new(std::time::Duration::from_millis(if closing { CLOSE_MS } else { OPEN_MS })).with_easing(
                    {
                        let quint = ease_out_quint();
                        move |t| if closing { ease_in_out(t) } else { quint(t) }
                    },
                ),
                move |d, t| {
                    let t = if closing { 1. - t } else { t };
                    let lerp = |a: f32, b: f32| a + (b - a) * t;
                    d.left(px(lerp(fx, tx)))
                        .top(px(lerp(fy, ty)))
                        .w(px(lerp(fw, w)))
                        .h(px(lerp(fh, h)))
                        // The menu fades as it lands, leaving the card itself.
                        .when(closing, |d| d.opacity(0.35 + 0.65 * t))
                },
            );

        let stage = div()
            .id("context-stage")
            .relative()
            .w(px(vw))
            .h(px(vh))
            .occlude()
            .child(div().absolute().inset_0().bg(hsla(0., 0., 0., 0.45)).with_animation(
                SharedString::from(format!("context-dim-{}-{key}-{open_id}", if closing { "out" } else { "in" })),
                Animation::new(std::time::Duration::from_millis(if closing { CLOSE_MS } else { OPEN_MS })),
                move |d, t| d.opacity(if closing { 1. - t } else { t }),
            ))
            .child(card)
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.close_context(cx)))
            .on_mouse_down(MouseButton::Right, cx.listener(|this, _, _, cx| this.close_context(cx)));

        Some(deferred(anchored().position(point(px(0.), px(0.))).child(stage)).with_priority(3).into_any_element())
    }

    /// Galleries, inline under "Save to gallery": tick to add or remove.
    fn render_context_galleries(&mut self, item: &Item, c: Hsla, cx: &mut Context<Self>) -> AnyElement {
        let p = pal();
        let key = entry_of(item).key();
        let galleries: Vec<(u64, String, bool)> = self
            .galleries
            .list
            .iter()
            .filter(|g| g.source == Source::Manual)
            .map(|g| (g.id, g.name.clone(), g.contains(&key)))
            .collect();
        let rows = galleries.into_iter().map(|(id, name, on)| {
            let item = item.clone();
            h_flex()
                .id(("ctx-gallery", id))
                .gap_2()
                .pl(px(34.))
                .pr_2()
                .h(px(28.))
                .rounded_md()
                .cursor_pointer()
                .text_sm()
                .text_color(p.muted)
                .hover(move |s| s.bg(with_alpha(c, 0.12)).text_color(p.fg))
                .child(
                    div().flex_none().size(px(14.)).flex().items_center().justify_center().rounded_sm().border_1().map(
                        |d| {
                            if on {
                                d.bg(c).border_color(c).child(icon("check").size(px(10.)).text_color(white()))
                            } else {
                                d.border_color(p.border_strong)
                            }
                        },
                    ),
                )
                .child(div().flex_1().min_w_0().truncate().child(name))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.galleries.toggle(id, entry_of(&item));
                    if let Err(e) = this.galleries.save() {
                        this.set_status(format!("Saving galleries failed: {e:#}"), true, cx);
                    }
                    cx.notify();
                }))
        });
        let new_item = item.clone();
        v_flex()
            .gap_0p5()
            .children(rows)
            .child(
                h_flex()
                    .id("ctx-gallery-new")
                    .gap_2()
                    .pl(px(34.))
                    .h(px(28.))
                    .rounded_md()
                    .cursor_pointer()
                    .text_sm()
                    .text_color(p.dim)
                    .hover(move |s| s.bg(with_alpha(c, 0.12)).text_color(p.fg))
                    .child(icon("plus").size_3p5())
                    .child("New gallery")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let id = this.new_gallery(Some(new_item.clone()), cx);
                        let name = this.galleries.get(id).map(|g| g.name.clone()).unwrap_or_default();
                        this.set_status(format!("Added to {name}"), false, cx);
                    })),
            )
            .into_any_element()
    }
}
