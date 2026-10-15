//! The right-click card: the wallpaper up top in its own colors, your
//! screens to put it on, then its actions as rows with shortcut keys.
//! Galleries open inline. Click outside or press Esc to close.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::galleries::entry_of;
use super::{
    Item, Muralist, Placement, boxed, file_name, glass, gridmodes, icon, keycap, pal, purity_color, purity_label,
    radius, with_alpha,
};
use crate::gallery::Source;

const CARD_W: f32 = 304.;
const PAD: f32 = 10.;
const HERO_H: f32 = 156.;
const DESK_H: f32 = 52.;

pub(super) struct ContextMenu {
    pub item: Item,
    pub at: Point<Pixels>,
}

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
    pub(super) fn open_context(&mut self, item: Item, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.picker = None;
        self.context_hover = None;
        self.context_galleries = false;
        self.context = Some(ContextMenu { item, at });
        cx.notify();
    }

    pub(super) fn close_context(&mut self, cx: &mut Context<Self>) {
        if self.context.take().is_some() {
            self.context_hover = None;
            self.context_galleries = false;
            cx.notify();
        }
    }

    fn run_context(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(action, Action::Galleries) {
            self.context_galleries = !self.context_galleries;
            cx.notify();
            return;
        }
        let Some(menu) = self.context.take() else { return };
        self.context_hover = None;
        self.context_galleries = false;
        let item = menu.item;
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
        let Some(menu) = &self.context else { return false };
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
        let (item, at) = (menu.item.clone(), menu.at);
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
        let inner_w = CARD_W - PAD * 2.;

        // ---- hero ----
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
                            .bg(with_alpha(pc, 0.25))
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
        let hero = div()
            .relative()
            .w(px(inner_w))
            .h(px(HERO_H))
            .map(|d| radius(d, 10.))
            .bg(p.bg)
            .when_some(thumb.clone(), |d, t| {
                d.child(radius(boxed(img(t), inner_w, HERO_H).object_fit(ObjectFit::Cover), 10.))
            })
            .child(
                v_flex()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .px_3()
                    .pt_8()
                    .pb_2p5()
                    .gap_1()
                    .map(|d| super::radius_bottom(d, 10.))
                    .bg(linear_gradient(
                        0.,
                        linear_color_stop(hsla(0., 0., 0., 0.75), 0.),
                        linear_color_stop(hsla(0., 0., 0., 0.), 1.),
                    ))
                    .child(
                        div().text_sm().font_weight(FontWeight::SEMIBOLD).text_color(white()).truncate().child(title),
                    )
                    .child(h_flex().gap_2().text_xs().text_color(hsla(0., 0., 1., 0.8)).children(chips)),
            );
        let palette = (!colors.is_empty()).then(|| {
            h_flex()
                .gap_1()
                .h(px(6.))
                .children(colors.iter().take(6).map(|col| div().flex_1().h_full().rounded_full().bg(*col)))
        });

        // ---- set on: the real desk, small ----
        let desk = can_set.then(|| {
            let min_x = self.monitors.iter().map(|m| m.x).min().unwrap_or(0);
            let min_y = self.monitors.iter().map(|m| m.y).min().unwrap_or(0);
            let max_x = self.monitors.iter().map(|m| m.x + m.width as i32).max().unwrap_or(1);
            let max_y = self.monitors.iter().map(|m| m.y + m.height as i32).max().unwrap_or(1);
            let all_w = 44.;
            let scale =
                ((inner_w - all_w - 8.) / (max_x - min_x).max(1) as f32).min(DESK_H / (max_y - min_y).max(1) as f32);
            let gap = 4.;
            let screens =
                self.monitors.clone().into_iter().enumerate().map(|(i, m)| {
                    let (w, h) = (m.width as f32 * scale - gap, m.height as f32 * scale - gap);
                    let hovered = self.context_hover == Some(i);
                    let name = m.name.clone();
                    div()
                        .id(("ctx-screen", i))
                        .absolute()
                        .left(px((m.x - min_x) as f32 * scale + gap / 2.))
                        .top(px((m.y - min_y) as f32 * scale + gap / 2.))
                        .w(px(w))
                        .h(px(h))
                        .map(|d| radius(d, 6.))
                        .bg(p.bg)
                        .cursor_pointer()
                        // Hover ghosts the wallpaper onto that screen.
                        .when_some(thumb.clone().filter(|_| hovered), |d, t| {
                            d.child(radius(boxed(img(t), w, h).object_fit(ObjectFit::Cover), 6.))
                        })
                        .child(
                            h_flex()
                                .absolute()
                                .inset_0()
                                .justify_center()
                                .items_center()
                                .gap_1()
                                .text_size(px(10.))
                                .text_color(if hovered { white() } else { p.muted })
                                .when(!hovered, |d| d.child(div().text_color(p.dim).child(format!("{}", i + 1))))
                                .child(m.name.clone()),
                        )
                        .child(div().absolute().inset_0().map(|d| radius(d, 6.)).border_1().border_color(if hovered {
                            c
                        } else {
                            p.border_strong
                        }))
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
            let all = div()
                .id("ctx-all")
                .flex_none()
                .w(px(all_w))
                .h(px((max_y - min_y) as f32 * scale - gap))
                .flex()
                .items_center()
                .justify_center()
                .rounded_md()
                .border_1()
                .border_color(p.border_strong)
                .text_xs()
                .text_color(p.muted)
                .cursor_pointer()
                .hover(move |s| s.border_color(c).text_color(p.fg))
                .child("All")
                .on_click(cx.listener(|this, _, window, cx| this.run_context(Action::SetAll, window, cx)));
            v_flex()
                .gap_1p5()
                .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(p.dim).child("SET ON"))
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .relative()
                                .flex_none()
                                .w(px((max_x - min_x) as f32 * scale))
                                .h(px((max_y - min_y) as f32 * scale))
                                .children(screens),
                        )
                        .child(div().flex_1())
                        .child(all),
                )
        });

        // ---- actions ----
        let galleries_open = self.context_galleries;
        let gallery_list = self.context_galleries.then(|| self.render_context_galleries(&item, c, cx));
        let row =
            |id: &'static str, icon_name: &'static str, label: String, hint: Option<&'static str>, action: Action| {
                h_flex()
                    .id(id)
                    .gap_2p5()
                    .px_2()
                    .h(px(32.))
                    .rounded_md()
                    .cursor_pointer()
                    .text_sm()
                    .text_color(p.fg)
                    .hover(move |s| s.bg(with_alpha(c, 0.16)))
                    .child(icon(icon_name).size_4().text_color(c))
                    .child(div().flex_1().child(label))
                    .children(hint.map(|h| keycap(h, p)))
                    .on_click(cx.listener(move |this, _, window, cx| this.run_context(action.clone(), window, cx)))
            };
        let remote_id = match &item {
            Item::Remote(w) => Some(w.id.clone()),
            Item::Local(_) => None,
        };
        let mut rows: Vec<AnyElement> = Vec::new();
        if can_set {
            rows.push(
                row("ctx-set", "wallpaper", "Set as wallpaper".into(), Some("S"), Action::SetAll).into_any_element(),
            );
            if self.monitors.len() > 1 {
                rows.push(
                    row("ctx-span", "frame", "Span across screens".into(), Some("P"), Action::Span).into_any_element(),
                );
            }
        }
        rows.push(
            row("ctx-galleries", "bookmark", "Save to gallery".into(), Some("G"), Action::Galleries)
                .child(icon(if galleries_open { "chevron-down" } else { "chevron-right" }).size_3p5().text_color(p.dim))
                .into_any_element(),
        );
        rows.extend(gallery_list);
        if let Some(id) = remote_id {
            rows.push(
                row("ctx-similar", "search", "Find similar".into(), Some("F"), Action::Similar(id)).into_any_element(),
            );
            rows.push(
                row("ctx-download", "arrow-down", "Save to library".into(), Some("D"), Action::Download)
                    .into_any_element(),
            );
        }
        rows.push(row("ctx-open", "maximize-2", "Open".into(), Some("Enter"), Action::Open).into_any_element());

        let card = v_flex()
            .id("context-card")
            .w(px(CARD_W))
            .p(px(PAD))
            .gap_2p5()
            .rounded_xl()
            .bg(linear_gradient(
                180.,
                linear_color_stop(Hsla { l: 0.16, s: c.s.min(0.5), a: 0.98, ..c }, 0.),
                linear_color_stop(with_alpha(p.elevated, 0.98), 0.45),
            ))
            .border_1()
            .border_color(with_alpha(c, 0.45))
            .shadow_lg()
            .occlude()
            .child(hero)
            .children(palette)
            .children(desk)
            .child(div().h(px(1.)).bg(with_alpha(c, 0.25)))
            .child(v_flex().gap_0p5().children(rows))
            .on_mouse_down_out(cx.listener(|this, _, _, cx| this.close_context(cx)))
            .with_animation(
                SharedString::from(format!("context-open-{key}")),
                Animation::new(std::time::Duration::from_millis(160)).with_easing(ease_out_quint()),
                |d, t| d.opacity(t).mt(px(-6. * (1. - t))),
            );

        Some(
            deferred(anchored().position(at).snap_to_window_with_margin(px(8.)).child(card))
                .with_priority(3)
                .into_any_element(),
        )
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
