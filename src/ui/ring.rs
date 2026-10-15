//! The context ring: right-click a wallpaper and its actions fly out on an
//! orbit around it, in the wallpaper's own color. The wallpaper sits in the
//! middle; hovering an action names it there.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::galleries::Picker;
use super::{Item, Muralist, Placement, file_name, glass, gridmodes, icon, pal, with_alpha};

/// Orbit radius, button size, and the margin around the orbit.
const ORBIT: f32 = 96.;
const BUTTON: f32 = 44.;
const MARGIN: f32 = 34.;
const CORE: f32 = 78.;

pub(super) struct Ring {
    pub item: Item,
    pub at: Point<Pixels>,
}

#[derive(Clone)]
enum Action {
    SetAll,
    SetOn(String),
    Span,
    Gallery,
    Similar(String),
    Download,
    Open,
}

impl Action {
    fn icon(&self) -> &'static str {
        match self {
            Action::SetAll => "wallpaper",
            Action::SetOn(_) => "monitor",
            Action::Span => "frame",
            Action::Gallery => "bookmark",
            Action::Similar(_) => "search",
            Action::Download => "arrow-down",
            Action::Open => "maximize-2",
        }
    }

    fn label(&self) -> String {
        match self {
            Action::SetAll => "Set as wallpaper".into(),
            Action::SetOn(name) => format!("Set on {name}"),
            Action::Span => "Span across screens".into(),
            Action::Gallery => "Save to gallery".into(),
            Action::Similar(_) => "Find similar".into(),
            Action::Download => "Save to library".into(),
            Action::Open => "Open".into(),
        }
    }

    /// The short tag shown on monitor buttons.
    fn tag(&self) -> Option<String> {
        match self {
            Action::SetOn(name) => Some(name.clone()),
            _ => None,
        }
    }
}

impl Muralist {
    pub(super) fn open_ring(&mut self, item: Item, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.picker = None;
        self.ring_hover = None;
        self.ring = Some(Ring { item, at });
        cx.notify();
    }

    pub(super) fn close_ring(&mut self, cx: &mut Context<Self>) {
        if self.ring.take().is_some() {
            self.ring_hover = None;
            cx.notify();
        }
    }

    fn ring_actions(&self, item: &Item) -> Vec<Action> {
        let can_set = self.backend.is_supported() && !self.monitors.is_empty();
        let mut actions = Vec::new();
        if can_set {
            actions.push(Action::SetAll);
            actions.extend(self.monitors.iter().map(|m| Action::SetOn(m.name.clone())));
            if self.monitors.len() > 1 {
                actions.push(Action::Span);
            }
        }
        actions.push(Action::Gallery);
        if let Item::Remote(w) = item {
            actions.push(Action::Similar(w.id.clone()));
            actions.push(Action::Download);
        }
        actions.push(Action::Open);
        actions
    }

    fn run_ring_action(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ring) = self.ring.take() else { return };
        self.ring_hover = None;
        let item = ring.item;
        let targets = self.target.targets();
        match action {
            Action::SetAll => self.assign_item_then(item, targets, Placement::All, true, cx),
            Action::SetOn(name) => self.assign_item_then(item, targets, Placement::Monitor(name), true, cx),
            Action::Span => {
                self.select(item, window, cx);
                self.open_span_editor(cx);
            }
            Action::Gallery => self.picker = Some(Picker { item, at: ring.at }),
            Action::Similar(id) => self.search_for(format!("like:{id}"), window, cx),
            Action::Download => {
                self.select(item, window, cx);
                self.download_selected(cx);
            }
            Action::Open => {
                self.select(item, window, cx);
                self.open_lightbox(window, cx);
            }
        }
        cx.notify();
    }

    pub(super) fn render_ring(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = pal();
        let ring = self.ring.as_ref()?;
        let (item, at) = (ring.item.clone(), ring.at);
        let c = glass::glow_color(&gridmodes::item_colors(&item, &self.local_palettes)).unwrap_or(p.accent);
        let thumb = match &item {
            Item::Remote(w) => self.remote_thumb(w, false),
            Item::Local(path) => self.local_thumb(path),
        };
        let actions = self.ring_actions(&item);
        let n = actions.len();
        let hovered = self.ring_hover.and_then(|i| actions.get(i)).map(Action::label);
        let caption = hovered.clone().unwrap_or_else(|| match &item {
            Item::Remote(w) => w.id.clone(),
            Item::Local(path) => file_name(path),
        });

        let size = (ORBIT + BUTTON / 2. + MARGIN) * 2.;
        let mid = size / 2.;
        let key = match &item {
            Item::Remote(w) => w.id.clone(),
            Item::Local(path) => file_name(path),
        };

        let buttons = actions.into_iter().enumerate().map(|(i, action)| {
            let angle = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::TAU / n as f32;
            let on = self.ring_hover == Some(i);
            let tag = action.tag();
            div()
                .id(("ring-action", i))
                .absolute()
                .size(px(BUTTON))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(if on { with_alpha(c, 0.3) } else { with_alpha(p.elevated, 0.96) })
                .border_1()
                .border_color(if on { c } else { with_alpha(c, 0.45) })
                .text_color(if on { p.fg } else { p.muted })
                .cursor_pointer()
                .shadow_md()
                .child(icon(action.icon()).size_4())
                .when_some(tag, |d, tag| {
                    d.child(
                        div()
                            .absolute()
                            .bottom(px(-7.))
                            .px_1()
                            .rounded_sm()
                            .bg(hsla(0., 0., 0., 0.75))
                            .text_size(px(9.))
                            .text_color(white())
                            .child(tag),
                    )
                })
                .on_hover(cx.listener(move |this, hovering: &bool, _, cx| {
                    let next = if *hovering { Some(i) } else { this.ring_hover.filter(|h| *h != i) };
                    if this.ring_hover != next {
                        this.ring_hover = next;
                        cx.notify();
                    }
                }))
                .on_click(cx.listener(move |this, _, window, cx| this.run_ring_action(action.clone(), window, cx)))
                // Fly out from the middle, one after another.
                .with_animation(
                    SharedString::from(format!("ring-{key}-{i}")),
                    Animation::new(std::time::Duration::from_millis(340)),
                    move |d, t| {
                        let lag = i as f32 * 0.05;
                        let t = ((t - lag) / (1. - lag).max(0.01)).clamp(0., 1.);
                        // Ease out with a little overshoot.
                        let e = 1. + 2.2 * (t - 1.).powi(3) + 1.2 * (t - 1.).powi(2);
                        let r = ORBIT * e;
                        d.left(px(mid + r * angle.cos() - BUTTON / 2.))
                            .top(px(mid + r * angle.sin() - BUTTON / 2.))
                            .opacity(t.min(1.))
                    },
                )
        });

        let core = v_flex()
            .absolute()
            .left(px(mid - CORE / 2.))
            .top(px(mid - CORE / 2.))
            .size(px(CORE))
            .rounded_full()
            .overflow_hidden()
            .border_2()
            .border_color(c)
            .bg(p.bg)
            .when_some(thumb, |d, t| d.child(img(t).size_full().object_fit(ObjectFit::Cover)));

        let panel = div()
            .id("context-ring")
            .relative()
            .size(px(size))
            .occlude()
            // A dark halo so the ring reads over any wallpaper.
            .child(
                div()
                    .absolute()
                    .left(px(MARGIN / 2.))
                    .top(px(MARGIN / 2.))
                    .size(px(size - MARGIN))
                    .rounded_full()
                    .bg(hsla(0., 0., 0., 0.42)),
            )
            // The orbit.
            .child(
                div()
                    .absolute()
                    .left(px(mid - ORBIT))
                    .top(px(mid - ORBIT))
                    .size(px(ORBIT * 2.))
                    .rounded_full()
                    .border_1()
                    .border_color(with_alpha(c, 0.3)),
            )
            .child(core)
            .child(
                h_flex().absolute().left_0().right_0().top(px(mid + CORE / 2. + 6.)).justify_center().child(
                    div()
                        .max_w(px(ORBIT * 1.4))
                        .truncate()
                        .px_2()
                        .py_0p5()
                        .rounded_full()
                        .bg(hsla(0., 0., 0., 0.7))
                        .text_xs()
                        .text_color(if hovered.is_some() { white() } else { hsla(0., 0., 1., 0.7) })
                        .child(caption),
                ),
            )
            .children(buttons)
            .on_mouse_down_out(cx.listener(|this, _, _, cx| this.close_ring(cx)))
            .with_animation(
                SharedString::from(format!("ring-open-{key}")),
                Animation::new(std::time::Duration::from_millis(160)).with_easing(ease_out_quint()),
                |d, t| d.opacity(t),
            );

        let origin = point(at.x - px(mid), at.y - px(mid));
        Some(deferred(anchored().position(origin).snap_to_window().child(panel)).with_priority(3).into_any_element())
    }
}
