//! Info and Similar for the selected wallpaper, in a modal: the picture
//! large on the left, the details on the right. Same window language as
//! Settings; Esc or a click outside closes it.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    InspectorTab, Item, Muralist, details, file_name, kit, pal, purity_color, purity_label, radius, section, segment,
    segmented_track, with_alpha,
};

const SHEET_W: f32 = 1040.;
const SHEET_H: f32 = 680.;
/// The details pane on the right.
const PANE_W: f32 = 420.;
const PANE_PAD: f32 = 22.;

impl Muralist {
    pub(super) fn open_sheet(&mut self, tab: InspectorTab, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.is_none() {
            return;
        }
        self.inspector_tab = tab;
        self.focus.focus(window, cx);
        cx.notify();
    }

    fn close_sheet(&mut self, cx: &mut Context<Self>) {
        self.inspector_tab = InspectorTab::Set;
        cx.notify();
    }

    pub(super) fn sheet_open(&self) -> bool {
        self.inspector_tab != InspectorTab::Set && self.selected.is_some()
    }

    pub(super) fn render_sheet(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.sheet_open() {
            return None;
        }
        let p = pal();
        let item = self.selected.clone()?;
        let tab = self.inspector_tab;
        let remote = matches!(item, Item::Remote(_));

        // The picture: the full original once it's here, the large thumbnail
        // until then.
        let picture = match &item {
            Item::Remote(w) => self.remote_full(w, cx).or_else(|| self.remote_thumb(w, true)),
            Item::Local(path) => Some(path.clone()),
        };
        let (title, facts): (String, Vec<AnyElement>) = match &item {
            Item::Remote(w) => {
                let pc = purity_color(&w.purity);
                (
                    w.id.clone(),
                    vec![
                        h_flex()
                            .gap_1()
                            .px_2()
                            .py_0p5()
                            .rounded_full()
                            .bg(with_alpha(pc, 0.2))
                            .text_color(pc)
                            .child(div().size(px(6.)).rounded_full().bg(pc))
                            .child(purity_label(&w.purity))
                            .into_any_element(),
                        div().child(w.resolution.clone()).into_any_element(),
                        div().child(w.category.clone()).into_any_element(),
                    ],
                )
            }
            Item::Local(path) => (file_name(path), Vec::new()),
        };
        let left = div()
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .p_5()
            .bg(hsla(0., 0., 0., 0.35))
            .child(
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .when_some(picture, |d, t| d.child(radius(img(t).size_full().object_fit(ObjectFit::Contain), 10.))),
            )
            .child(
                h_flex()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .px_6()
                    .pt_10()
                    .pb_5()
                    .gap_2p5()
                    .text_xs()
                    .text_color(hsla(0., 0., 1., 0.85))
                    .bg(linear_gradient(
                        0.,
                        linear_color_stop(hsla(0., 0., 0., 0.7), 0.),
                        linear_color_stop(hsla(0., 0., 0., 0.), 1.),
                    ))
                    .child(div().text_base().font_weight(FontWeight::SEMIBOLD).text_color(white()).child(title.clone()))
                    .children(facts),
            );

        let similar_count = match &item {
            Item::Remote(w) => match self.similar.get(&w.id) {
                Some(details::Similar::Ready(list)) => Some(list.len()),
                _ => None,
            },
            _ => None,
        };
        let tab_seg = |id: &'static str, label: String, t: InspectorTab| {
            segment(id, label, tab == t, p).px_3().on_click(cx.listener(move |this, _, _, cx| {
                this.inspector_tab = t;
                cx.notify();
            }))
        };
        let tabs =
            segmented_track(p).child(tab_seg("sheet-info", "Info".into(), InspectorTab::Info)).when(remote, |d| {
                d.child(tab_seg(
                    "sheet-similar",
                    match similar_count {
                        Some(n) if n > 0 => format!("Similar  {n}"),
                        _ => "Similar".into(),
                    },
                    InspectorTab::Similar,
                ))
            });

        // The detail views size themselves from `details_w`.
        let saved_w = self.details_w;
        self.details_w = PANE_W - PANE_PAD * 2.;
        let content: AnyElement = match (tab, &item) {
            (InspectorTab::Similar, Item::Remote(w)) => {
                let w = w.clone();
                self.render_similar(&w, cx).unwrap_or_else(|| {
                    div().text_sm().text_color(p.dim).child("No similar wallpapers found").into_any_element()
                })
            }
            (_, Item::Remote(w)) => {
                let w = w.clone();
                self.render_info_tab(&w, cx)
            }
            (_, Item::Local(path)) => {
                let path = path.clone();
                let palette = self.render_xray_palette(cx);
                v_flex()
                    .gap_5()
                    .children(palette)
                    .child(
                        section("File", p).child(div().text_xs().text_color(p.muted).child(path.display().to_string())),
                    )
                    .into_any_element()
            }
        };
        self.details_w = saved_w;

        let right = v_flex()
            .flex_none()
            .w(px(PANE_W))
            .h_full()
            .bg(p.surface)
            .border_l_1()
            .border_color(p.border)
            .child(
                h_flex()
                    .flex_none()
                    .h(px(64.))
                    .px(px(PANE_PAD))
                    .gap_3()
                    .border_b_1()
                    .border_color(p.border)
                    .child(tabs)
                    .child(div().flex_1())
                    .child(
                        kit::Btn::new("sheet-close")
                            .icon("close")
                            .ghost()
                            .sm()
                            .tooltip("Close (Esc)")
                            .on_click(cx.listener(|this, _, _, cx| this.close_sheet(cx))),
                    ),
            )
            .child(
                div()
                    .id(SharedString::from(format!("sheet-body-{title}-{}", tab as u8)))
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(div().p(px(PANE_PAD)).child(content)),
            );

        let card = h_flex()
            .id("sheet-card")
            .w(px(SHEET_W))
            .h(px(SHEET_H))
            .max_w(relative(0.94))
            .max_h(relative(0.9))
            .rounded_xl()
            .overflow_hidden()
            .bg(p.bg)
            .border_1()
            .border_color(p.border_strong)
            .shadow_lg()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(left)
            .child(right)
            .with_animation(
                "sheet-open",
                Animation::new(std::time::Duration::from_millis(200)).with_easing(ease_out_quint()),
                |d, t| d.opacity(t).mt(px(14. * (1. - t))),
            );

        Some(
            div()
                .id("sheet-overlay")
                .track_focus(&self.focus)
                .occlude()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(hsla(0., 0., 0., 0.6))
                .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                    if ev.keystroke.key == "escape" {
                        this.close_sheet(cx);
                        cx.stop_propagation();
                    }
                }))
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.close_sheet(cx)))
                .child(card)
                .into_any_element(),
        )
    }
}
