//! The preview while its original is on the wire: a camera hunting for
//! focus over the developing thumbnail. Corner brackets breathe in and out,
//! a beam sweeps the frame, a telemetry chip maps which byte ranges have
//! landed, the palette is "sampled" down the edge as bytes arrive, and the
//! percentage counts up big. When the last byte lands the brackets snap to
//! the edges and the frame flashes: locked.

use std::time::Duration;

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{Bands, with_alpha};

const ARM: f32 = 16.;
const THICK: f32 = 2.;

/// One L-shaped corner bracket, `inset` in from its corner.
fn bracket(corner: usize, inset: f32, c: Hsla) -> Div {
    let (left, top) = (corner % 2 == 0, corner < 2);
    let arm = |horizontal: bool| {
        let d = div().absolute().bg(c);
        let d = if horizontal { d.w(px(ARM)).h(px(THICK)) } else { d.w(px(THICK)).h(px(ARM)) };
        let d = if left { d.left_0() } else { d.right_0() };
        if top { d.top_0() } else { d.bottom_0() }
    };
    let d = div().absolute().size(px(ARM)).child(arm(true)).child(arm(false));
    let d = if left { d.left(px(inset)) } else { d.right(px(inset)) };
    if top { d.top(px(inset)) } else { d.bottom(px(inset)) }
}

fn mb(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / 1_048_576.)
}

/// Layers over a preview `h` tall whose original is still downloading.
pub(super) fn hud(key: &str, bands: Bands, colors: &[Hsla], c: Hsla, h: f32) -> Vec<AnyElement> {
    let Bands { fraction, mask, count, done, total } = bands;
    let f = fraction.unwrap_or(0.);
    let mut layers = Vec::new();

    // Rule-of-thirds guides.
    let guide = hsla(0., 0., 1., 0.1);
    for i in 1..3 {
        let at = relative(i as f32 / 3.);
        layers.push(div().absolute().top_0().bottom_0().left(at).w(px(1.)).bg(guide).into_any_element());
        layers.push(div().absolute().left_0().right_0().top(at).h(px(1.)).bg(guide).into_any_element());
    }

    // The beam, sweeping down over and over.
    const BAND: f32 = 46.;
    layers.push(
        div()
            .absolute()
            .left_0()
            .right_0()
            .h(px(BAND))
            .child(div().absolute().inset_0().bg(linear_gradient(
                180.,
                linear_color_stop(with_alpha(c, 0.), 0.),
                linear_color_stop(with_alpha(c, 0.28), 1.),
            )))
            .child(div().absolute().left_0().right_0().bottom_0().h(px(1.5)).bg(with_alpha(c, 0.9)))
            .with_animation(
                SharedString::from(format!("dev-beam-{key}")),
                Animation::new(Duration::from_millis(1300)).repeat().with_easing(ease_in_out),
                move |d, t| d.top(px(-BAND + t * (h + BAND))),
            )
            .into_any_element(),
    );

    // Brackets hunting for focus.
    for corner in 0..4 {
        layers.push(
            bracket(corner, 14., c)
                .with_animation(
                    SharedString::from(format!("dev-hunt-{key}-{corner}")),
                    Animation::new(Duration::from_millis(900)).repeat().with_easing(pulsating_between(0., 1.)),
                    move |_, t| bracket(corner, 8. + 10. * t, c),
                )
                .into_any_element(),
        );
    }

    // Center reticle, its ring pulsing.
    layers.push(
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .relative()
                    .size(px(22.))
                    .child(div().absolute().left(px(10.)).top(px(4.)).w(px(2.)).h(px(14.)).bg(with_alpha(c, 0.7)))
                    .child(div().absolute().top(px(10.)).left(px(4.)).h(px(2.)).w(px(14.)).bg(with_alpha(c, 0.7)))
                    .child(div().absolute().inset_0().rounded_full().border_1().border_color(c).with_animation(
                        SharedString::from(format!("dev-ring-{key}")),
                        Animation::new(Duration::from_millis(1200)).repeat(),
                        |d, t| d.opacity(1. - t).top(px(-8. * t)).left(px(-8. * t)).size(px(22. + 16. * t)),
                    )),
            )
            .into_any_element(),
    );

    // Telemetry: which byte ranges have landed, and how much.
    let n = if count > 0 { count } else { 16 };
    let lit = if count > 0 { mask } else { (1u64 << ((f * 16.).floor() as u32).min(63)) - 1 };
    let dots = (0..n.min(32)).map(|i| {
        let on = lit & (1u64 << i) != 0;
        div().w(px(4.)).h(px(8.)).rounded_sm().bg(if on { c } else { hsla(0., 0., 1., 0.18) })
    });
    let status = if total == 0 { "CONNECTING".to_owned() } else { format!("RECV {} / {} MB", mb(done), mb(total)) };
    layers.push(
        v_flex()
            .absolute()
            .top_3()
            .left_3()
            .gap_1()
            .px_2()
            .py_1p5()
            .rounded_md()
            .bg(hsla(0., 0., 0., 0.55))
            .border_1()
            .border_color(with_alpha(c, 0.35))
            .child(h_flex().gap(px(2.)).children(dots))
            .child(
                h_flex()
                    .gap_1p5()
                    .text_size(px(9.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(hsla(0., 0., 1., 0.85))
                    .child(div().size(px(5.)).rounded_full().bg(c).with_animation(
                        SharedString::from(format!("dev-rec-{key}")),
                        Animation::new(Duration::from_millis(700)).repeat().with_easing(pulsating_between(0.2, 1.)),
                        |d, t| d.opacity(t),
                    ))
                    .child(status)
                    .when(count > 1, |d| d.child(div().text_color(hsla(0., 0., 1., 0.5)).child(format!("x{count}")))),
            )
            .into_any_element(),
    );

    // The palette, sampled down the right edge as the bytes come in.
    let sampled = ((f * colors.len() as f32).ceil() as usize).min(colors.len());
    layers.push(
        v_flex()
            .absolute()
            .top_3()
            .right_3()
            .gap_1()
            .children(colors.iter().take(sampled).enumerate().map(|(i, col)| {
                div().size(px(10.)).rounded_sm().bg(*col).border_1().border_color(hsla(0., 0., 1., 0.5)).with_animation(
                    SharedString::from(format!("dev-sample-{key}-{i}")),
                    Animation::new(Duration::from_millis(380)).with_easing(ease_out_quint()),
                    |d, t| d.opacity(t).ml(px(14. * (1. - t))),
                )
            }))
            .into_any_element(),
    );

    // The count, big.
    layers.push(
        h_flex()
            .absolute()
            .left_3()
            .bottom(px(18.))
            .items_end()
            .gap_0p5()
            .text_color(white())
            .child(
                div()
                    .text_size(px(26.))
                    .line_height(px(26.))
                    .font_weight(FontWeight::BOLD)
                    .child(fraction.map_or("--".to_owned(), |f| format!("{}", (f * 100.) as u32))),
            )
            .child(div().pb_0p5().text_xs().text_color(hsla(0., 0., 1., 0.6)).child("%"))
            .into_any_element(),
    );
    // And a hairline under everything, filling.
    layers.push(
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .h(px(3.))
            .bg(hsla(0., 0., 1., 0.1))
            .child(div().h_full().w(relative(f.max(0.01))).bg(c))
            .into_any_element(),
    );
    layers
}

/// The moment the last byte lands: brackets snap out to the edges, the
/// frame flashes, and a chip names what arrived.
pub(super) fn lock(key: &str, c: Hsla, label: String) -> Vec<AnyElement> {
    let mut layers = vec![
        div()
            .absolute()
            .inset_0()
            .bg(white())
            .with_animation(
                SharedString::from(format!("dev-flash-{key}")),
                Animation::new(Duration::from_millis(520)).with_easing(ease_out_quint()),
                |d, t| d.opacity(0.3 * (1. - t)),
            )
            .into_any_element(),
    ];
    for corner in 0..4 {
        layers.push(
            bracket(corner, 8., c)
                .with_animation(
                    SharedString::from(format!("dev-lock-{key}-{corner}")),
                    Animation::new(Duration::from_millis(700)).with_easing(ease_out_quint()),
                    move |_, t| bracket(corner, 8. - 6. * t.min(0.4) / 0.4, c).opacity(1. - t),
                )
                .into_any_element(),
        );
    }
    layers.push(
        div()
            .absolute()
            .left_3()
            .bottom_3()
            .child(
                h_flex()
                    .gap_1p5()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .bg(hsla(0., 0., 0., 0.6))
                    .border_1()
                    .border_color(with_alpha(c, 0.6))
                    .text_size(px(10.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(white())
                    .child(div().size(px(5.)).rounded_full().bg(c))
                    .child(label),
            )
            .with_animation(
                SharedString::from(format!("dev-locked-{key}")),
                Animation::new(Duration::from_millis(1800)),
                |d, t| {
                    // In fast, hold, then out.
                    let o = if t < 0.12 {
                        t / 0.12
                    } else if t > 0.7 {
                        (1. - t) / 0.3
                    } else {
                        1.
                    };
                    d.opacity(o)
                },
            )
            .into_any_element(),
    );
    layers
}
