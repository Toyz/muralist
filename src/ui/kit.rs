//! muralist's controls: one button used everywhere, plus the shared surface
//! treatment (soft vertical sheen, hairline top highlight, tactile press).
//! No glow: light in muralist comes from wallpapers, not from chrome.

use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Sizable as _, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{Pal, icon, on_accent, pal, with_alpha};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Tone {
    Primary,
    Secondary,
    Ghost,
    Danger,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Size {
    Sm,
    Md,
    Lg,
}

impl Size {
    fn height(self) -> f32 {
        match self {
            Size::Sm => 28.,
            Size::Md => 34.,
            Size::Lg => 42.,
        }
    }
}

type ClickHandler = Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>;

pub(super) struct Btn {
    id: ElementId,
    label: Option<SharedString>,
    icon: Option<SharedString>,
    tooltip: Option<SharedString>,
    tone: Tone,
    size: Size,
    full: bool,
    round: bool,
    disabled: bool,
    loading: bool,
    selected: bool,
    on_click: Option<ClickHandler>,
}

impl Btn {
    pub(super) fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            label: None,
            icon: None,
            tooltip: None,
            tone: Tone::Secondary,
            size: Size::Md,
            full: false,
            round: false,
            disabled: false,
            loading: false,
            selected: false,
            on_click: None,
        }
    }

    pub(super) fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }
    pub(super) fn icon(mut self, name: impl Into<SharedString>) -> Self {
        self.icon = Some(name.into());
        self
    }
    pub(super) fn tooltip(mut self, text: impl Into<SharedString>) -> Self {
        self.tooltip = Some(text.into());
        self
    }
    pub(super) fn tone(mut self, tone: Tone) -> Self {
        self.tone = tone;
        self
    }
    pub(super) fn primary(self) -> Self {
        self.tone(Tone::Primary)
    }
    pub(super) fn ghost(self) -> Self {
        self.tone(Tone::Ghost)
    }
    pub(super) fn danger(self) -> Self {
        self.tone(Tone::Danger)
    }
    pub(super) fn sm(mut self) -> Self {
        self.size = Size::Sm;
        self
    }
    pub(super) fn lg(mut self) -> Self {
        self.size = Size::Lg;
        self
    }
    pub(super) fn full(mut self) -> Self {
        self.full = true;
        self
    }
    pub(super) fn round(mut self) -> Self {
        self.round = true;
        self
    }
    pub(super) fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    pub(super) fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }
    pub(super) fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
    pub(super) fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Box::new(f));
        self
    }

    pub(super) fn render(self) -> Stateful<Div> {
        let p = pal();
        let h = self.size.height();
        let icon_only = self.label.is_none();
        let (fill, fill_hover, text, edge) = colors(self.tone, self.selected, p);
        let interactive = !self.disabled && !self.loading;
        let icon_size = if self.size == Size::Sm { px(13.) } else { px(15.) };

        let mut el = h_flex()
            .id(self.id)
            .relative()
            .flex_none()
            .justify_center()
            .gap_2()
            .h(px(h))
            .map(|d| if icon_only { d.w(px(h)) } else { d.px(px(if self.size == Size::Sm { 10. } else { 16. })) })
            .when(self.full, gpui_kit::Styled::w_full)
            .map(|d| if self.round { d.rounded_full() } else { d.rounded_lg() })
            .overflow_hidden()
            .text_color(text)
            .map(|d| match self.size {
                Size::Sm => d.text_xs(),
                Size::Md => d.text_sm(),
                Size::Lg => d.text_sm().font_weight(FontWeight::SEMIBOLD),
            })
            .when(self.size != Size::Lg, |d| d.font_weight(FontWeight::MEDIUM))
            .when_some(fill, gpui_kit::Styled::bg)
            .when_some(edge, |d, e| d.border_1().border_color(e))
            // Lit bevel: a hairline of light along the top edge.
            .when(self.tone != Tone::Ghost || self.selected, |d| {
                d.child(div().absolute().top_0().left(px(6.)).right(px(6.)).h(px(1.)).bg(hsla(
                    0.,
                    0.,
                    1.,
                    if self.tone == Tone::Primary { 0.32 } else { 0.07 },
                )))
            })
            .when_some(self.loading.then_some(()), |d, _| d.child(Spinner::new().xsmall().color(text)))
            .when_some(self.icon.filter(|_| !self.loading), |d, name| d.child(icon(&name).size(icon_size)))
            .when_some(self.label, gpui_kit::ParentElement::child)
            .when_some(self.tooltip, |d, t| d.tooltip(move |window, cx| Tooltip::new(t.clone()).build(window, cx)));

        if interactive {
            el = el
                .cursor_pointer()
                .hover(move |s| {
                    let s = match fill_hover {
                        Some(f) => s.bg(f),
                        None => s,
                    };
                    s.text_color(match self.tone {
                        Tone::Ghost | Tone::Secondary => p.fg,
                        _ => text,
                    })
                })
                // Tactile press: the face dips a pixel.
                .active(|s| s.pt(px(2.)).opacity(0.92));
            if let Some(f) = self.on_click {
                el = el.on_click(f);
            }
        } else if self.disabled {
            el = el.opacity(0.4).cursor_default();
        } else {
            // Loading: busy, not unavailable, so keep full color.
            el = el.cursor_default();
        }
        el
    }
}

impl IntoElement for Btn {
    type Element = Stateful<Div>;

    fn into_element(self) -> Self::Element {
        self.render()
    }
}

/// Fill, hover fill, text and border for a tone.
fn colors(tone: Tone, selected: bool, p: Pal) -> (Option<Fill>, Option<Fill>, Hsla, Option<Hsla>) {
    let sheen = |top: Hsla, bottom: Hsla| -> Fill {
        linear_gradient(180., linear_color_stop(top, 0.), linear_color_stop(bottom, 1.)).into()
    };
    let lift = |c: Hsla, by: f32| Hsla { l: (c.l + by).min(1.), ..c };
    if selected {
        let tint = with_alpha(p.accent, 0.16);
        return (Some(tint.into()), Some(with_alpha(p.accent, 0.24).into()), p.fg, Some(with_alpha(p.accent, 0.6)));
    }
    match tone {
        Tone::Primary => (
            Some(sheen(lift(p.accent, 0.06), p.accent)),
            Some(sheen(lift(p.accent, 0.12), lift(p.accent, 0.04))),
            on_accent(p.accent),
            Some(Hsla { l: (p.accent.l * 0.7).max(0.), ..p.accent }),
        ),
        Tone::Danger => (
            Some(sheen(lift(p.error, 0.05), p.error)),
            Some(sheen(lift(p.error, 0.1), lift(p.error, 0.03))),
            on_accent(p.error),
            Some(Hsla { l: p.error.l * 0.7, ..p.error }),
        ),
        Tone::Secondary => (
            Some(sheen(p.elevated, p.surface2)),
            Some(sheen(lift(p.elevated, 0.03), p.elevated)),
            p.fg,
            Some(p.border_strong),
        ),
        Tone::Ghost => (None, Some(p.surface2.into()), p.muted, None),
    }
}

/// The shared surface for tiles and chips: same sheen and bevel as buttons.
pub(super) fn surface(d: Div, p: Pal) -> Div {
    d.relative()
        .bg(linear_gradient(
            180.,
            linear_color_stop(with_alpha(p.elevated, 0.9), 0.),
            linear_color_stop(with_alpha(p.surface2, 0.9), 1.),
        ))
        .border_1()
        .border_color(p.border)
        .child(div().absolute().top_0().left(px(6.)).right(px(6.)).h(px(1.)).bg(hsla(0., 0., 1., 0.06)))
}
