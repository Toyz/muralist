mod adaptive;
mod details;
mod drag;
mod galleries;
mod glass;
mod gridmodes;
mod history;
mod kit;
mod lightbox;
mod peek;
mod rotate;
mod searchbar;
mod settings;
mod slot;
mod smartq;
mod span_editor;
mod stage;

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use anyhow::Context as _;
use futures::StreamExt as _;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Icon, Sizable as _, Theme, ThemeTokens, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::backend::{Reach, Session};
use crate::config::{Config, FilterOverrides, GridMode, LockMode, State, cache_dir};
use crate::gallery::{self as gallery_entry, Galleries};
use crate::monitors::Monitor;
use crate::span;
use crate::thumbs::{Job, ThumbPool, remote_cache_path};
use crate::wallhaven::{Client, Progress, SearchParams, Sorting, TOP_RANGES, UserSettings, Wallpaper, WallpaperInfo};

const SIDEBAR_W: f32 = 264.;
/// Content width of the sidebar's Info view.
const DETAILS_W: f32 = INSPECTOR_W - INSPECTOR_PAD * 2.;
const TITLE_H: f32 = 56.;
const CARD_PAD: f32 = 16.;
/// Sidebar cards are narrower, so a little tighter.
const SIDE_CARD_PAD: f32 = 12.;
const INSPECTOR_W: f32 = 420.;
const INSPECTOR_PAD: f32 = 20.;
/// Preview height when nothing (or nothing measurable) is selected.
const PREVIEW_H: f32 = 236.;

/// Preview box height that matches an image's shape: short for ultrawides,
/// tall for portraits, clamped so it never takes over the sidebar.
fn preview_h_for(aspect: f32) -> f32 {
    (DETAILS_W / aspect.max(0.1)).clamp(150., 440.).round()
}
/// Seconds of pointer travel to extrapolate when predicting the target card.
const TOAST_FOR: std::time::Duration = std::time::Duration::from_millis(3500);
const PRECOG_LOOKAHEAD: f32 = 0.2;
const PRECOG_DWELL_MS: u64 = 220;
const PRECOG_MAX_INFLIGHT: usize = 3;
/// Speculative fetches use fewer connections than one you asked for.
const PRECOG_WORKERS: usize = 6;
const SELECTED_WORKERS: usize = 12;
/// Tags shown before the "+N more" toggle.
const TAG_LIMIT: usize = 10;
const GRID_PAD: f32 = 20.;
const GRID_GAP: f32 = 14.;
const MIN_CARD_W: f32 = 250.;
const SCROLLBAR_W: f32 = 2.;

const RATIO_PRESETS: [(&str, Option<&str>); 3] =
    [("Any", None), ("16:9", Some("16x9,16x10")), ("Ultrawide", Some("21x9,32x9,48x9"))];
const RES_PRESETS: [(&str, Option<&str>); 4] =
    [("Any", None), ("1080p", Some("1920x1080")), ("1440p", Some("2560x1440")), ("4K", Some("3840x2160"))];
const PURITIES: [&str; 3] = ["SFW", "Sketchy", "NSFW"];
const CATEGORIES: [&str; 3] = ["General", "Anime", "People"];

// ---- palette ----------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
struct Pal {
    bg: Hsla,
    surface: Hsla,
    surface2: Hsla,
    elevated: Hsla,
    border: Hsla,
    border_strong: Hsla,
    fg: Hsla,
    muted: Hsla,
    dim: Hsla,
    accent: Hsla,
    accent_hover: Hsla,
    ok: Hsla,
    error: Hsla,
}

/// 13059 -> "13,059"
fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// Text color that reads on the accent: dark on bright accents (yellows,
/// tans), white on deep ones.
fn on_accent(accent: Hsla) -> Hsla {
    let c: Rgba = accent.into();
    let lum = 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
    if lum > 0.42 { hsla(0., 0., 0.07, 1.) } else { hsla(0., 0., 1., 1.) }
}

/// Straight blend of two colors, `t` from 0 (a) to 1 (b).
fn mix(a: Hsla, b: Hsla, t: f32) -> Hsla {
    let (a, b): (Rgba, Rgba) = (a.into(), b.into());
    Rgba { r: a.r + (b.r - a.r) * t, g: a.g + (b.g - a.g) * t, b: a.b + (b.b - a.b) * t, a: 1. }.into()
}

fn hex(c: u32) -> Hsla {
    rgb(c).into()
}

fn with_alpha(c: Hsla, a: f32) -> Hsla {
    Hsla { a, ..c }
}

/// The live palette (see `adaptive`): stock colors, or ones taken from the
/// selected wallpaper.
fn pal() -> Pal {
    adaptive::current()
}

fn purity_color(purity: &str) -> Hsla {
    match purity {
        "sfw" | "SFW" => hex(0x22c55e),
        "sketchy" | "Sketchy" => hex(0xf59e0b),
        _ => hex(0xef4444),
    }
}

fn purity_label(purity: &str) -> &'static str {
    match purity {
        "sfw" => "SFW",
        "sketchy" => "Sketchy",
        _ => "NSFW",
    }
}

fn parse_hex(s: &str) -> Option<Hsla> {
    u32::from_str_radix(s.trim_start_matches('#'), 16).ok().map(hex)
}

/// Point gpui-component's theme (inputs, buttons, scrollbars) at our palette.
pub fn apply_theme(cx: &mut App) {
    let p = pal();
    let t = Theme::global_mut(cx);
    t.background = p.bg;
    t.foreground = p.fg;
    t.border = p.border;
    t.input = p.border_strong;
    t.ring = p.accent;
    t.caret = p.accent;
    t.selection = with_alpha(p.accent, 0.35);
    t.muted = p.surface2;
    t.muted_foreground = p.muted;
    t.accent = p.elevated;
    t.accent_foreground = p.fg;
    t.popover = p.surface2;
    t.popover_foreground = p.fg;
    t.primary = p.accent;
    t.primary_hover = p.accent_hover;
    t.primary_active = p.accent;
    t.primary_foreground = on_accent(p.accent);
    t.button_primary = p.accent;
    t.button_primary_hover = p.accent_hover;
    t.button_primary_active = p.accent;
    t.button_primary_foreground = on_accent(p.accent);
    t.secondary = p.elevated;
    t.secondary_hover = p.border_strong;
    t.secondary_active = p.border_strong;
    t.secondary_foreground = p.fg;
    t.button = p.elevated;
    t.button_hover = p.border_strong;
    t.button_active = p.border_strong;
    t.button_foreground = p.fg;
    t.radius = px(8.);
    t.radius_lg = px(12.);
    // Components read the derived token set, so re-derive it from the colors.
    t.tokens = ThemeTokens::from(&**t);
}

// ---- small building blocks --------------------------------------------------

/// Wallhaven's color search palette.
const WALLHAVEN_COLORS: [&str; 29] = [
    "660000", "990000", "cc0000", "cc3333", "ea4c88", "993399", "663399", "333399", "0066cc", "0099cc", "66cccc",
    "77cc33", "669900", "336600", "666600", "999900", "cccc33", "ffff00", "ffcc33", "ff9900", "ff6600", "cc6633",
    "996633", "663300", "000000", "999999", "cccccc", "ffffff", "424153",
];

/// GPUI clips children to a rectangle, not to rounded corners, so every layer
/// touching a rounded edge must carry its own radius.
fn radius<E: Styled>(mut e: E, r: f32) -> E {
    e.style().corner_radii = CornersRefinement {
        top_left: Some(px(r).into()),
        top_right: Some(px(r).into()),
        bottom_right: Some(px(r).into()),
        bottom_left: Some(px(r).into()),
    };
    e
}

fn radius_bottom<E: Styled>(mut e: E, r: f32) -> E {
    let c = &mut e.style().corner_radii;
    c.bottom_left = Some(px(r).into());
    c.bottom_right = Some(px(r).into());
    e
}

/// Radius of a `rounded_xl` box minus its 2px border.
const CARD_INNER_R: f32 = 10.;

fn icon(name: &str) -> Icon {
    Icon::empty().path(format!("icons/{name}.svg"))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BtnKind {
    Primary,
    Secondary,
}

fn button(id: impl Into<ElementId>, label: impl Into<SharedString>, kind: BtnKind, _p: Pal) -> Stateful<Div> {
    kit::Btn::new(id)
        .label(label)
        .tone(match kind {
            BtnKind::Primary => kit::Tone::Primary,
            BtnKind::Secondary => kit::Tone::Secondary,
        })
        .render()
}

fn small_button(id: impl Into<ElementId>, label: impl Into<SharedString>, kind: BtnKind) -> Stateful<Div> {
    kit::Btn::new(id)
        .label(label)
        .tone(match kind {
            BtnKind::Primary => kit::Tone::Primary,
            BtnKind::Secondary => kit::Tone::Secondary,
        })
        .sm()
        .render()
}

fn icon_button(id: impl Into<ElementId>, name: &str, _p: Pal) -> Stateful<Div> {
    kit::Btn::new(id).icon(name.to_owned()).ghost().sm().render()
}

/// Equal-width option tile for the filter grids. `tint` colors it (purity).
fn opt(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    on: bool,
    tint: Option<Hsla>,
    p: Pal,
) -> Stateful<Div> {
    let c = tint.unwrap_or(p.accent);
    kit::surface(h_flex(), p)
        .id(id)
        .min_w_0()
        .justify_center()
        .gap_1p5()
        .h(px(28.))
        .px_1()
        .rounded_md()
        .overflow_hidden()
        .text_xs()
        .cursor_pointer()
        .active(|s| s.pt(px(2.)))
        .map(|d| {
            if on {
                d.bg(with_alpha(c, 0.16))
                    .border_color(with_alpha(c, 0.55))
                    .text_color(if tint.is_some() { c } else { p.fg })
                    .font_weight(FontWeight::MEDIUM)
            } else {
                d.text_color(p.muted).hover(move |s| s.border_color(p.border_strong).text_color(p.fg))
            }
        })
        .when_some(tint, |d, c| d.child(div().flex_none().size(px(6.)).rounded_full().bg(c)))
        .child(div().truncate().child(label.into()))
}

fn opt_grid(cols: u16) -> Div {
    div().grid().grid_cols(cols).gap_1p5()
}

/// Segmented control item (lives inside a `segmented_track`).
fn segment(id: impl Into<ElementId>, label: impl Into<SharedString>, on: bool, p: Pal) -> Stateful<Div> {
    div()
        .id(id)
        .flex_1()
        .flex()
        .justify_center()
        .px_2()
        .py_1p5()
        .rounded_md()
        .text_xs()
        .cursor_pointer()
        .map(|d| {
            if on {
                d.bg(p.elevated).text_color(p.fg).font_weight(FontWeight::MEDIUM).shadow_sm()
            } else {
                d.text_color(p.muted).hover(move |s| s.text_color(p.fg))
            }
        })
        .child(label.into())
}

fn segmented_track(p: Pal) -> Div {
    h_flex().p_1().gap_1().rounded_lg().bg(p.bg).border_1().border_color(p.border)
}

/// A titled card in the sidebar, in the same language as the inspector's
/// cards. `summary` sits at the right of the header: the card's current value.
fn side_card(icon_name: &str, title: &str, summary: Option<SharedString>, p: Pal) -> Div {
    side_card_with(icon_name, title, p, |h| {
        h.children(summary.map(|s| div().min_w_0().text_xs().text_color(p.dim).truncate().child(s)))
    })
}

/// [`side_card`] with a custom right side of the header.
fn side_card_with(icon_name: &str, title: &str, p: Pal, right: impl FnOnce(Div) -> Div) -> Div {
    let head = h_flex()
        .gap_2()
        .child(icon(icon_name).size_4().flex_none().text_color(p.accent))
        .child(div().flex_none().text_sm().font_weight(FontWeight::SEMIBOLD).child(title.to_owned()))
        .child(div().flex_1());
    v_flex()
        .gap_3()
        .p(px(SIDE_CARD_PAD))
        .rounded_xl()
        .bg(with_alpha(p.surface2, 0.82))
        .border_1()
        .border_color(p.border)
        .child(right(head).min_w_0().gap_2())
}

/// The faint color wash behind a top-bar field, from the dominant colors of
/// what the tab is showing.
fn field_wash(tints: &[Hsla], p: Pal) -> Background {
    let (a, b) = match tints {
        [] => (p.surface2, p.surface2),
        [only] => (*only, p.surface2),
        [first, .., last] => (*first, *last),
    };
    let wash = |c: Hsla| Hsla { l: (c.l * 0.35).min(0.2), s: (c.s * 0.8).min(0.6), a: 1., ..c };
    linear_gradient(90., linear_color_stop(wash(a), 0.), linear_color_stop(wash(b), 1.))
}

/// Every tab's top bar leads with a field shaped like the search box.
fn bar_field(id: &'static str, tints: &[Hsla], p: Pal) -> Stateful<Div> {
    h_flex()
        .id(id)
        .flex_1()
        .min_w_0()
        .h(px(38.))
        .gap_2()
        .px_3()
        .rounded_lg()
        .border_1()
        .border_color(p.border_strong)
        .bg(field_wash(tints, p))
}

/// A path as clickable breadcrumbs, home shortened to `~`.
fn path_crumbs(id: &'static str, path: &Path, p: Pal) -> Stateful<Div> {
    let (root, rest) = match dirs::home_dir().and_then(|h| path.strip_prefix(&h).ok().map(Path::to_path_buf)) {
        Some(rel) => ("~".to_owned(), rel),
        None => (String::new(), path.to_path_buf()),
    };
    let parts: Vec<String> = std::iter::once(root)
        .chain(rest.components().filter_map(|c| match c {
            std::path::Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        }))
        .filter(|s| !s.is_empty())
        .collect();
    let last = parts.len().saturating_sub(1);
    h_flex()
        .id(id)
        .min_w_0()
        .overflow_hidden()
        .gap_1()
        .px_1p5()
        .h(px(24.))
        .rounded_md()
        .text_sm()
        .cursor_pointer()
        .hover(move |s| s.bg(with_alpha(p.elevated, 0.8)))
        .children(parts.into_iter().enumerate().flat_map(move |(i, part)| {
            let sep = (i > 0).then(|| div().flex_none().text_color(p.dim).child("/").into_any_element());
            let name = div()
                .flex_none()
                .text_color(if i == last { p.fg } else { p.muted })
                .when(i == last, |d| d.font_weight(FontWeight::MEDIUM))
                .child(part)
                .into_any_element();
            sep.into_iter().chain(std::iter::once(name))
        }))
}

/// A key, drawn like a keycap.
fn keycap(label: &'static str, p: Pal) -> Div {
    div().px_1().rounded_sm().border_1().border_color(p.border_strong).text_xs().text_color(p.dim).child(label)
}

/// A small label over a group inside a card.
fn sub_label(title: &str, p: Pal) -> Div {
    v_flex().gap_1p5().child(div().text_xs().text_color(p.dim).child(title.to_owned()))
}

fn section(title: &str, p: Pal) -> Div {
    v_flex()
        .gap_2()
        .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).text_color(p.dim).child(title.to_uppercase()))
}

fn switch(on: bool, p: Pal) -> Div {
    h_flex()
        .flex_none()
        .w(px(30.))
        .h(px(18.))
        .p(px(2.))
        .rounded_full()
        .bg(if on { p.accent } else { p.border_strong })
        .when(on, |d| d.justify_end())
        .child(div().size(px(14.)).rounded_full().bg(white()))
}

/// A bar of the custom window frame: drag to move, double-click to maximize,
/// right-click for the compositor's window menu. Windows does all of that
/// natively once the bar is marked as a drag area.
fn title_region(d: Div, p: Pal) -> Div {
    d.h(px(TITLE_H))
        .flex_none()
        .window_control_area(WindowControlArea::Drag)
        .border_b_1()
        .border_color(p.border)
        .on_mouse_down(MouseButton::Left, |ev: &MouseDownEvent, window, _| {
            if ev.click_count >= 2 {
                window.zoom_window();
            } else {
                window.start_window_move();
            }
        })
        .on_mouse_down(MouseButton::Right, |ev: &MouseDownEvent, window, _| window.show_window_menu(ev.position))
}

/// Keeps clicks on interactive children from starting a window drag. The
/// occlusion matters on Windows, where the drag area otherwise wins the hit
/// test over everything inside it.
fn no_drag<E: InteractiveElement>(e: E) -> E {
    e.occlude().on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

/// macOS draws its own window buttons at the top left.
const NATIVE_WINDOW_BUTTONS: bool = cfg!(target_os = "macos");

fn window_controls(window: &Window, p: Pal) -> impl IntoElement {
    let ctrl = |id: &'static str, name: &str, danger: bool, area: WindowControlArea| {
        no_drag(
            div()
                .id(id)
                .window_control_area(area)
                .size(px(28.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_md()
                .text_color(p.muted)
                .cursor_pointer()
                .hover(
                    move |s| {
                        if danger { s.bg(p.error).text_color(white()) } else { s.bg(p.elevated).text_color(p.fg) }
                    },
                )
                .child(icon(name).size_3p5()),
        )
    };
    let max_icon = if window.is_maximized() { "window-restore" } else { "window-maximize" };
    h_flex()
        .gap_1()
        .child(
            ctrl("win-min", "window-minimize", false, WindowControlArea::Min)
                .on_click(|_, window, _| window.minimize_window()),
        )
        .child(ctrl("win-max", max_icon, false, WindowControlArea::Max).on_click(|_, window, _| window.zoom_window()))
        .child(ctrl("win-close", "window-close", true, WindowControlArea::Close).on_click(|_, _, cx| cx.quit()))
}

// ---- loading treatment --------------------------------------------------------

/// Where the selected original is in its download.
#[derive(Clone)]
enum FullPhase {
    Done,
    Loading(Bands),
    Failed(String),
}

/// Pointer-trajectory prediction state for speculative fetching.
#[derive(Default)]
struct Precog {
    last: Option<(Point<Pixels>, Instant)>,
    /// Smoothed pointer velocity in px/s.
    vel: (f32, f32),
    predicted: Option<usize>,
    streak: u8,
    /// Top speed since the current prediction began.
    peak: f32,
    hover: Option<usize>,
    speculative: VecDeque<String>,
    prefetched: HashSet<String>,
    started: u32,
    hits: u32,
}

/// Which horizontal bands of the original have landed.
#[derive(Clone, Copy)]
struct Bands {
    fraction: Option<f32>,
    mask: u64,
    count: u32,
}

/// One band per downloaded byte range, in the picture's own colors, each
/// flashing as its bytes land. Ranges are fetched out of order over parallel
/// connections, so the image develops in a ragged wave, stragglers and all.
fn develop_bands(key: &str, src: &Path, bands: Bands, r: f32) -> Vec<AnyElement> {
    let Bands { fraction, mask, count } = bands;
    // Single-stream downloads have no ranges; fake 16 sequential bands.
    let (mask, n) = match (count, fraction) {
        (0, Some(f)) => {
            let filled = (f * 16.).floor() as u32;
            (if filled >= 64 { u64::MAX } else { (1u64 << filled) - 1 }, 16)
        }
        (0, None) => (0, 1),
        (c, _) => (mask, c),
    };
    let accent = pal().accent;
    (0..n)
        .filter(|i| mask & (1u64 << i) != 0)
        .map(|i| {
            let band = 1. / n as f32;
            div()
                .absolute()
                .left_0()
                .right_0()
                .top(relative(i as f32 * band))
                .h(relative(band))
                .overflow_hidden()
                .child(
                    // Full-size image shifted up so this band shows its own slice.
                    div()
                        .absolute()
                        .left_0()
                        .w_full()
                        .top(relative(-(i as f32)))
                        .h(relative(n as f32))
                        .child(radius(img(src.to_owned()).size_full().object_fit(ObjectFit::Contain), r)),
                )
                .child(div().absolute().top_0().left_0().size_full().bg(white()).with_animation(
                    SharedString::from(format!("band-flash-{key}-{i}")),
                    Animation::new(std::time::Duration::from_millis(650)).with_easing(ease_out_quint()),
                    |d, t| d.opacity(0.4 * (1. - t)),
                ))
                .child(div().absolute().bottom_0().left_0().right_0().h(px(1.5)).bg(accent).with_animation(
                    SharedString::from(format!("band-line-{key}-{i}")),
                    Animation::new(std::time::Duration::from_millis(900)).with_easing(ease_out_quint()),
                    |d, t| d.opacity(1. - t),
                ))
                .into_any_element()
        })
        .collect()
}

fn accent_gradient(p: Pal) -> Background {
    linear_gradient(90., linear_color_stop(p.accent, 0.), linear_color_stop(hex(0x22d3ee), 1.))
}

/// Thin glowing line: fills with `fraction`, or sweeps while the size is unknown.
fn progress_line(id: impl Into<ElementId>, fraction: Option<f32>, height: f32, p: Pal) -> AnyElement {
    let track = div().relative().w_full().h(px(height)).rounded_full().bg(hsla(0., 0., 1., 0.12));
    match fraction {
        Some(f) => track
            .child(div().h_full().w(relative(f.clamp(0.02, 1.))).rounded_full().bg(accent_gradient(p)))
            .into_any_element(),
        None => track
            .child(
                div()
                    .absolute()
                    .top_0()
                    .h_full()
                    .w(relative(0.3))
                    .rounded_full()
                    .bg(accent_gradient(p))
                    .with_animation(
                        id,
                        Animation::new(std::time::Duration::from_millis(1100)).repeat().with_easing(ease_in_out),
                        |d, t| d.left(relative(-0.3 + t * 1.3)),
                    ),
            )
            .into_any_element(),
    }
}

/// Soft band of light sweeping across a still-loading image.
fn shimmer(id: impl Into<ElementId>) -> impl IntoElement {
    let light = hsla(0., 0., 1., 0.10);
    let clear = hsla(0., 0., 1., 0.);
    h_flex()
        .absolute()
        .top_0()
        .bottom_0()
        .w(relative(0.5))
        .child(div().flex_1().h_full().bg(linear_gradient(
            90.,
            linear_color_stop(clear, 0.),
            linear_color_stop(light, 1.),
        )))
        .child(div().flex_1().h_full().bg(linear_gradient(
            90.,
            linear_color_stop(light, 0.),
            linear_color_stop(clear, 1.),
        )))
        .with_animation(
            id,
            Animation::new(std::time::Duration::from_millis(1800)).repeat().with_easing(ease_in_out),
            |d, t| d.left(relative(-0.5 + t * 1.5)),
        )
}

/// Wraps the full-resolution layer so it develops in over the thumbnail.
fn fade_in(id: impl Into<ElementId>, child: impl IntoElement) -> impl IntoElement {
    div().absolute().top_0().left_0().size_full().child(child).with_animation(
        id,
        Animation::new(std::time::Duration::from_millis(500)).with_easing(ease_out_quint()),
        |d, t| d.opacity(t),
    )
}

// ---- model --------------------------------------------------------------------

#[derive(Clone)]
enum KeyTest {
    Idle,
    Testing,
    Ok(String),
    Failed(String),
}

fn dir_size(path: &Path) -> u64 {
    std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            _ => e.metadata().map(|m| m.len()).unwrap_or(0),
        })
        .sum()
}

/// Download of a wallpaper's full-resolution original for the preview.
enum FullState {
    Loading { progress: Arc<Progress> },
    Ready(PathBuf),
    Failed(String),
}

/// Per-wallpaper details (tags, uploader) fetched on selection.
#[derive(Clone)]
enum InfoState {
    Loading,
    Ready(WallpaperInfo),
    Failed(String),
}

fn compact_count(n: u64) -> String {
    match n {
        0..1_000 => n.to_string(),
        1_000..1_000_000 => format!("{:.1}k", n as f64 / 1_000.),
        _ => format!("{:.1}M", n as f64 / 1_000_000.),
    }
}

/// Every search filter, as a value that can be diffed against a baseline.
#[derive(Clone, PartialEq)]
struct Filters {
    sorting: Sorting,
    top_range: String,
    ratios: Option<String>,
    atleast: Option<String>,
    resolutions: Option<String>,
    color: Option<String>,
    categories: [bool; 3],
    purity: [bool; 3],
}

impl Filters {
    fn with_overrides(mut self, o: &FilterOverrides, has_key: bool) -> Self {
        let opt = |v: &String| (!v.is_empty()).then(|| v.clone());
        if let Some(s) = o.sorting.as_deref().and_then(Sorting::from_api) {
            self.sorting = s;
        }
        if let Some(r) = o.top_range.as_ref().filter(|r| TOP_RANGES.contains(&r.as_str())) {
            self.top_range = r.clone();
        }
        if let Some(v) = &o.ratios {
            self.ratios = opt(v);
        }
        if let Some(v) = &o.atleast {
            self.atleast = opt(v);
        }
        if let Some(v) = &o.resolutions {
            self.resolutions = opt(v);
        }
        if let Some(v) = &o.color {
            self.color = opt(v);
        }
        if let Some(c) = o.categories.filter(|c| c.iter().any(|b| *b)) {
            self.categories = c;
        }
        if let Some(mut p) = o.purity {
            if !has_key {
                p[2] = false;
            }
            if p.iter().any(|b| *b) {
                self.purity = p;
            }
        }
        self
    }

    /// The fields of `self` that differ from `base`.
    fn diff(&self, base: &Filters) -> FilterOverrides {
        let s = |v: &Option<String>| v.clone().unwrap_or_default();
        FilterOverrides {
            sorting: (self.sorting != base.sorting).then(|| self.sorting.api().to_owned()),
            top_range: (self.top_range != base.top_range).then(|| self.top_range.clone()),
            ratios: (self.ratios != base.ratios).then(|| s(&self.ratios)),
            atleast: (self.atleast != base.atleast).then(|| s(&self.atleast)),
            resolutions: (self.resolutions != base.resolutions).then(|| s(&self.resolutions)),
            color: (self.color != base.color).then(|| s(&self.color)),
            categories: (self.categories != base.categories).then_some(self.categories),
            purity: (self.purity != base.purity).then_some(self.purity),
        }
    }
}

#[derive(Clone, Copy)]
struct GridLayout {
    cols: usize,
    card_w: f32,
    card_h: f32,
}

#[derive(Clone)]
enum Item {
    Remote(Wallpaper),
    Local(PathBuf),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Discover,
    Library,
    Galleries,
    History,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum InspectorTab {
    Set,
    Info,
    Similar,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Desktop,
    Lock,
}

impl Target {
    fn label(self) -> &'static str {
        match self {
            Target::Desktop => "Desktop",
            Target::Lock => "Lock screen",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TargetMode {
    Desktop,
    Lock,
    Both,
}

impl TargetMode {
    fn targets(self) -> Vec<Target> {
        match self {
            TargetMode::Desktop => vec![Target::Desktop],
            TargetMode::Lock => vec![Target::Lock],
            TargetMode::Both => vec![Target::Desktop, Target::Lock],
        }
    }
}

#[derive(Clone)]
enum Placement {
    Monitor(String),
    All,
    Span,
}

fn same_item(a: &Item, b: &Item) -> bool {
    match (a, b) {
        (Item::Remote(a), Item::Remote(b)) => a.id == b.id,
        (Item::Local(a), Item::Local(b)) => a == b,
        _ => false,
    }
}

fn local_key(path: &Path) -> String {
    format!("file:{}", path.display())
}

fn full_key(w: &Wallpaper) -> String {
    format!("full-{}", w.id)
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn is_image(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(),
        Some("jpg" | "jpeg" | "png" | "webp")
    )
}

pub struct Muralist {
    cfg: Config,
    /// The desktop integration (see `crate::backend`).
    backend: Arc<Session>,
    client: Client,
    monitors: Vec<Monitor>,
    account: Option<UserSettings>,

    query: Entity<InputState>,
    settings_open: bool,
    settings_section: settings::Section,
    key_input: Entity<InputState>,
    dir_input: Entity<InputState>,
    key_test: KeyTest,
    cache_size: u64,
    sorting: Sorting,
    top_range: String,
    ratios: Option<String>,
    atleast: Option<String>,
    resolutions: Option<String>,
    color: Option<String>,
    categories: [bool; 3],
    purity: [bool; 3],

    results: Vec<Wallpaper>,
    page: u32,
    last_page: u32,
    total: u64,
    seed: Option<String>,
    searching: bool,
    /// Bumped on every search so a slow, superseded response is dropped.
    search_gen: u64,
    grid_scroll: UniformListScrollHandle,
    grid_layout: GridLayout,

    library: Vec<PathBuf>,
    tab: Tab,
    galleries: Galleries,
    /// The gallery the Galleries tab shows.
    gallery: Option<u64>,
    gallery_work: HashMap<u64, galleries::GalleryWork>,
    import: Option<galleries::ImportState>,
    picker: Option<galleries::Picker>,
    /// Gallery whose name is being edited in the top bar.
    renaming: Option<u64>,
    /// Gallery whose delete button is waiting for a second click.
    confirm_delete: Option<u64>,
    gallery_name: Entity<InputState>,
    wh_user: Entity<InputState>,
    selected: Option<Item>,
    /// Holds the decoded full-resolution preview; cleared on every selection
    /// so only one large image stays in memory.
    preview_cache: Entity<RetainAllImageCache>,
    info: HashMap<String, InfoState>,
    full: HashMap<String, FullState>,
    tags_expanded: bool,
    precog: Precog,
    /// Width the Details/Info view lays out to (sidebar vs lightbox drawer).
    details_w: f32,
    inspector_tab: InspectorTab,
    /// A standalone search (tag / like / user), shown as a pill in the box.
    scope: Option<smartq::Scope>,
    /// Weighted palettes (RGB center, share) by image key, for Color X-ray.
    xray_palettes: HashMap<String, Vec<([f32; 3], f32)>>,
    /// Palette band under the pointer.
    xray: Option<usize>,
    similar: HashMap<String, details::Similar>,
    /// Tag id -> (name, category); `None` while fetching or if it failed.
    tag_names: HashMap<u64, Option<(String, String)>>,
    /// Query text of the last search sent, and a counter for type-ahead debounce.
    last_query: String,
    type_gen: u64,
    search_focused: bool,
    span_edit: Option<span_editor::SpanEdit>,
    rotating: bool,
    last_rotation: Instant,
    history_hover: Option<usize>,
    slot: Option<slot::Slot>,
    /// Spun wallpapers still downloading, by monitor.
    landing: Vec<(String, Wallpaper)>,
    /// Mosaic rows: per row, (item index, card width).
    justified: Vec<Vec<(usize, f32)>>,
    justified_h: f32,
    local_dims: HashMap<PathBuf, (u32, u32)>,
    /// Card under the pointer and where on it (0..1), for the holo glare.
    holo: Option<(usize, f32, f32)>,
    peek: Option<peek::Peek>,
    /// Focus of the main view, so Space reaches it when no field is focused.
    main_focus: FocusHandle,
    theme_anim: adaptive::ThemeAnim,
    local_palettes: HashMap<PathBuf, Vec<Hsla>>,
    /// Painted bounds of visible cards, keyed by item index.
    card_bounds: Rc<RefCell<HashMap<usize, Bounds<Pixels>>>>,
    /// Window scale factor and logical size, refreshed every frame.
    scale: f32,
    viewport: (f32, f32),
    /// Pixel size of the selected image, for the lightbox's 1:1 view.
    selected_dims: Option<(u32, u32)>,

    lightbox: bool,
    lb_zoom: bool,
    /// Direction of the last browse step (-1, 0 on open, 1) for the slide-in.
    lb_dir: i8,
    lb_desks: bool,
    lb_panel: bool,
    lb_keys: bool,
    /// Zoom explorer: logical px per image px, and the image point at center.
    lb_z: f32,
    lb_pan: (f32, f32),
    lb_mm_drag: bool,
    lb_loupe: bool,
    lb_cursor: Option<Point<Pixels>>,
    /// Window-space bounds of the displayed image, for the loupe.
    lb_rect: Rc<std::cell::Cell<Option<Bounds<Pixels>>>>,
    /// Content area (window minus client-side shadow), refreshed each frame.
    content: (f32, f32),
    lb_scroll: ScrollHandle,
    /// Last pointer position while dragging in the lightbox, and whether it moved.
    lb_drag: Option<(Point<Pixels>, bool)>,
    focus: FocusHandle,

    pool: ThumbPool,
    thumbs: HashMap<String, PathBuf>,
    /// When each thumbnail finished, so only fresh arrivals fade in.
    thumb_arrived: HashMap<String, Instant>,
    requested: HashSet<String>,

    target: TargetMode,
    desktop: BTreeMap<String, PathBuf>,
    lock: BTreeMap<String, PathBuf>,
    applied: State,

    busy: usize,
    status: SharedString,
    status_error: bool,
    /// When the current notice popped up; it fades after a few seconds.
    toast_at: Option<Instant>,
    _subscriptions: Vec<Subscription>,
    _thumb_task: Task<()>,
}

impl Muralist {
    pub fn new(cfg: Config, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let client = Client::new(cfg.effective_api_key()).expect("building HTTP client");
        let mut status: SharedString = "Ready".into();
        let mut status_error = false;

        let backend = Arc::new(Session::detect());
        if !backend.is_supported() {
            status = format!("muralist doesn't support {} yet", backend.name()).into();
            status_error = true;
        }
        let monitors = backend.monitors().unwrap_or_else(|e| {
            status = format!("Monitor detection failed: {e:#}").into();
            status_error = true;
            Vec::new()
        });

        let mut applied = State::load();
        if let Some(Ok(current)) = backend.desktop().and_then(|w| w.current(&monitors))
            && !current.is_empty()
        {
            applied.desktop = current;
        }
        if let Some(Ok(current)) = backend.lock().and_then(|w| w.current(&monitors))
            && !current.is_empty()
        {
            applied.lock = current;
        }

        let query = cx.new(|cx| InputState::new(window, cx).placeholder("Search tags, @username, id:123, like:abc123"));
        let key_input =
            cx.new(|cx| InputState::new(window, cx).masked(true).placeholder("Paste your Wallhaven API key"));
        let dir_input = cx.new(|cx| InputState::new(window, cx).placeholder("~/Pictures/muralist"));
        let sub = cx.subscribe_in(&query, window, |this, _, ev: &InputEvent, window, cx| match ev {
            InputEvent::PressEnter { .. } => {
                let raw = this.query.read(cx).value().to_string();
                if smartq::Scope::parse(&raw).is_some() {
                    this.search_for(raw, window, cx);
                } else {
                    this.search(true, cx);
                }
            }
            InputEvent::Change => {
                // Typing replaces a scope: Wallhaven can't combine them.
                if this.scope.is_some() && !this.query.read(cx).value().is_empty() {
                    this.scope = None;
                }
                this.search_soon(cx)
            }
            _ => {}
        });

        let gallery_name = cx.new(|cx| InputState::new(window, cx).placeholder("Gallery name"));
        let rename_sub = cx.subscribe_in(&gallery_name, window, |this, _, ev: &InputEvent, _, cx| match ev {
            InputEvent::PressEnter { .. } | InputEvent::Blur => this.commit_rename(cx),
            _ => {}
        });
        let wh_user =
            cx.new(|cx| InputState::new(window, cx).placeholder("username").default_value(cfg.wallhaven_user.clone()));

        let user_sub = cx.subscribe_in(&wh_user, window, |this, _, ev: &InputEvent, _, cx| match ev {
            InputEvent::PressEnter { .. } | InputEvent::Blur => this.commit_username(cx),
            _ => {}
        });

        let (pool, mut done) = ThumbPool::start(client.clone());
        let thumb_task = cx.spawn(async move |this, cx| {
            while let Some(first) = done.next().await {
                // Drain whatever else finished meanwhile so a burst of
                // thumbnails costs one re-render, not dozens.
                let mut batch = vec![first];
                while let Ok(more) = done.try_recv() {
                    batch.push(more);
                }
                let alive = this.update(cx, |this, cx| {
                    for (key, result) in batch {
                        match result {
                            Ok(path) => {
                                this.thumb_arrived.insert(key.clone(), Instant::now());
                                this.thumbs.insert(key, path);
                            }
                            Err(e) => eprintln!("thumbnail {key}: {e}"),
                        }
                    }
                    cx.notify();
                });
                if alive.is_err() {
                    break;
                }
            }
        });

        let main_focus = cx.focus_handle();
        main_focus.focus(window, cx);
        let mut this = Self {
            backend,
            client,
            monitors,
            account: None,
            query,
            settings_open: false,
            settings_section: settings::Section::default(),
            key_input,
            dir_input,
            key_test: KeyTest::Idle,
            cache_size: 0,
            sorting: Sorting::DateAdded,
            top_range: "1M".into(),
            ratios: None,
            atleast: None,
            resolutions: None,
            color: None,
            categories: [true, true, true],
            purity: [true, false, false],
            results: Vec::new(),
            page: 1,
            last_page: 1,
            total: 0,
            seed: None,
            searching: false,
            search_gen: 0,
            grid_scroll: UniformListScrollHandle::new(),
            grid_layout: GridLayout { cols: 1, card_w: MIN_CARD_W, card_h: MIN_CARD_W * 0.625 },
            library: Vec::new(),
            tab: Tab::Discover,
            galleries: Galleries::load(),
            gallery: None,
            gallery_work: HashMap::new(),
            import: None,
            picker: None,
            renaming: None,
            confirm_delete: None,
            gallery_name,
            wh_user,
            selected: None,
            preview_cache: RetainAllImageCache::new(cx),
            selected_dims: None,
            info: HashMap::new(),
            full: HashMap::new(),
            tags_expanded: false,
            precog: Precog::default(),
            details_w: DETAILS_W,
            inspector_tab: InspectorTab::Set,
            scope: None,
            xray_palettes: HashMap::new(),
            xray: None,
            similar: HashMap::new(),
            tag_names: HashMap::new(),
            last_query: String::new(),
            type_gen: 0,
            search_focused: false,
            span_edit: None,
            rotating: false,
            last_rotation: Instant::now(),
            history_hover: None,
            slot: None,
            landing: Vec::new(),
            justified: Vec::new(),
            justified_h: 200.,
            local_dims: HashMap::new(),
            holo: None,
            peek: None,
            main_focus,
            theme_anim: adaptive::ThemeAnim::default(),
            local_palettes: HashMap::new(),
            card_bounds: Rc::new(RefCell::new(HashMap::new())),
            scale: 1.,
            viewport: (1600., 1000.),
            lightbox: false,
            lb_zoom: false,
            lb_dir: 0,
            lb_desks: false,
            lb_panel: false,
            lb_keys: false,
            lb_z: 1.,
            lb_pan: (0., 0.),
            lb_mm_drag: false,
            lb_loupe: false,
            lb_cursor: None,
            lb_rect: Rc::new(std::cell::Cell::new(None)),
            content: (1600., 1000.),
            lb_scroll: ScrollHandle::new(),
            lb_drag: None,
            focus: cx.focus_handle(),
            pool,
            thumbs: HashMap::new(),
            thumb_arrived: HashMap::new(),
            requested: HashSet::new(),
            target: TargetMode::Desktop,
            desktop: applied.desktop.clone(),
            lock: applied.lock.clone(),
            applied,
            busy: 0,
            status,
            status_error,
            toast_at: None,
            _subscriptions: vec![sub, rename_sub, user_sub],
            _thumb_task: thumb_task,
            cfg,
        };
        this.restore_filters();
        this.refresh_library();
        this.start_rotation_clock(cx);
        this.resume_galleries(cx);
        if this.client.has_key() {
            this.load_account(cx);
        } else {
            this.status = format!("No API key; add api_key to {}", Config::path().display()).into();
            this.search(true, cx);
        }
        this
    }

    // ---- data -------------------------------------------------------------

    /// Show a notice. Successes fade after a few seconds; errors stay until
    /// dismissed or replaced.
    fn set_status(&mut self, s: impl Into<SharedString>, error: bool, cx: &mut Context<Self>) {
        self.status = s.into();
        self.status_error = error;
        let at = Instant::now();
        self.toast_at = Some(at);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TOAST_FOR).await;
            this.update(cx, |this, cx| {
                if this.toast_at == Some(at) && !this.status_error {
                    this.toast_at = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn render_toast(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = pal();
        let at = self.toast_at?;
        let error = self.status_error;
        let busy = self.busy > 0;
        let icon_el = if busy {
            Spinner::new().xsmall().color(p.accent).into_any_element()
        } else if error {
            icon("circle-alert").size_4().text_color(p.error).into_any_element()
        } else {
            icon("circle-check").size_4().text_color(p.ok).into_any_element()
        };
        // Centered under the grid; above the filmstrip in the lightbox.
        let bottom = if self.lightbox { 116. } else { 20. };
        let (left, right) = if self.lightbox { (0., 0.) } else { (SIDEBAR_W, INSPECTOR_W) };
        Some(
            h_flex()
                .absolute()
                .bottom(px(bottom))
                .left(px(left))
                .right(px(right))
                .justify_center()
                .child(
                    h_flex()
                        .id("toast")
                        .occlude()
                        .max_w(px(560.))
                        .gap_2p5()
                        .pl_3()
                        .pr(px(if error { 6. } else { 16. }))
                        .h(px(40.))
                        .rounded_full()
                        .bg(p.elevated)
                        .border_1()
                        .border_color(if error { with_alpha(p.error, 0.5) } else { p.border_strong })
                        .shadow_lg()
                        .text_sm()
                        .child(icon_el)
                        .child(div().min_w_0().truncate().child(self.status.clone()))
                        .when(error, |d| {
                            d.child(icon_button("toast-close", "close", p).size(px(28.)).rounded_full().on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.toast_at = None;
                                    this.status_error = false;
                                    cx.notify();
                                }),
                            ))
                        })
                        .with_animation(
                            SharedString::from(format!("toast-{at:?}")),
                            Animation::new(std::time::Duration::from_millis(260)).with_easing(ease_out_quint()),
                            |d, t| d.opacity(t).mb(px(-12. * (1. - t))),
                        ),
                )
                .into_any_element(),
        )
    }

    /// Fetch the account's Wallhaven settings and use them as the default filters.
    fn load_account(&mut self, cx: &mut Context<Self>) {
        let client = self.client.clone();
        self.set_status("Loading account settings...", false, cx);
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { client.settings() }).await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(settings) => {
                        this.account = Some(settings);
                        this.restore_filters();
                    }
                    Err(e) => this.set_status(format!("Account settings unavailable: {e:#}"), true, cx),
                }
                this.search(true, cx);
            })
            .ok();
        })
        .detach();
    }

    /// App defaults with the account's Wallhaven settings applied.
    fn baseline(&self) -> Filters {
        let mut f = Filters {
            sorting: Sorting::DateAdded,
            top_range: "1M".into(),
            ratios: None,
            atleast: None,
            resolutions: None,
            color: None,
            categories: [true, true, true],
            purity: [true, false, false],
        };
        if let Some(s) = &self.account {
            let purity = s.purity_flags();
            if purity.iter().any(|b| *b) {
                f.purity = purity;
            }
            let categories = s.category_flags();
            if categories.iter().any(|b| *b) {
                f.categories = categories;
            }
            if TOP_RANGES.contains(&s.toplist_range.as_str()) {
                f.top_range = s.toplist_range.clone();
            }
            f.ratios = s.ratios();
            f.resolutions = s.resolutions();
        }
        f
    }

    fn current_filters(&self) -> Filters {
        Filters {
            sorting: self.sorting,
            top_range: self.top_range.clone(),
            ratios: self.ratios.clone(),
            atleast: self.atleast.clone(),
            resolutions: self.resolutions.clone(),
            color: self.color.clone(),
            categories: self.categories,
            purity: self.purity,
        }
    }

    fn set_filters(&mut self, f: Filters) {
        self.sorting = f.sorting;
        self.top_range = f.top_range;
        self.ratios = f.ratios;
        self.atleast = f.atleast;
        self.resolutions = f.resolutions;
        self.color = f.color;
        self.categories = f.categories;
        self.purity = f.purity;
    }

    /// Baseline plus the overrides saved in the config.
    fn restore_filters(&mut self) {
        let f = self.baseline().with_overrides(&self.cfg.filters, self.client.has_key());
        self.set_filters(f);
    }

    /// Save whatever differs from the baseline, so it survives a restart.
    fn persist_filters(&mut self) {
        // Without the account loaded the baseline is incomplete, and diffing
        // against it would save account values as overrides.
        if self.client.has_key() && self.account.is_none() {
            return;
        }
        let overrides = self.current_filters().diff(&self.baseline());
        if overrides != self.cfg.filters {
            self.cfg.filters = overrides;
            if let Err(e) = self.cfg.save() {
                eprintln!("saving filters: {e:#}");
            }
        }
    }

    fn reset_filters(&mut self, cx: &mut Context<Self>) {
        self.cfg.filters = FilterOverrides::default();
        if let Err(e) = self.cfg.save() {
            eprintln!("saving filters: {e:#}");
        }
        self.restore_filters();
        self.search(true, cx);
    }

    fn select(&mut self, item: Item, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.as_ref().is_some_and(|s| same_item(s, &item)) {
            return;
        }
        if let Item::Remote(w) = &item {
            if self.precog.prefetched.remove(&w.id) {
                self.precog.hits += 1;
                self.precog.speculative.retain(|id| id != &w.id);
            }
        }
        // Moving on: stop the previous original's download so it doesn't
        // compete with the new one.
        if let Some(Item::Remote(prev)) = &self.selected {
            if let Some(FullState::Loading { progress, .. }) = self.full.get(&prev.id) {
                progress.cancel.store(true, Ordering::Relaxed);
                self.full.remove(&prev.id);
            }
        }
        self.preview_cache.update(cx, |cache, cx| cache.clear(window, cx));
        self.selected_dims = match &item {
            Item::Remote(w) => w.resolution.split_once('x').and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?))),
            Item::Local(path) => image::image_dimensions(path).ok(),
        };
        if let Item::Remote(w) = &item {
            self.load_info(w.id.clone(), false, cx);
            self.load_similar(w.id.clone(), cx);
        }
        self.xray = None;
        self.selected = Some(item);
        self.tags_expanded = false;
        self.lb_zoom = false;
        self.lb_scroll.set_offset(point(px(0.), px(0.)));
        cx.notify();
    }

    fn is_selected_id(&self, id: &str) -> bool {
        matches!(&self.selected, Some(Item::Remote(w)) if w.id == id)
    }

    /// Fetch tags/uploader for a wallpaper. Waits briefly first so arrow-key
    /// browsing doesn't spend the API's 45 requests/minute on skipped images.
    fn load_info(&mut self, id: String, force: bool, cx: &mut Context<Self>) {
        match self.info.get(&id) {
            Some(InfoState::Failed(_)) | None => {}
            Some(_) if !force => return,
            Some(_) => {}
        }
        self.info.insert(id.clone(), InfoState::Loading);
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(250)).await;
            let still_wanted = this
                .update(cx, |this, _| {
                    let wanted = this.is_selected_id(&id);
                    if !wanted {
                        this.info.remove(&id);
                    }
                    wanted
                })
                .unwrap_or(false);
            if !still_wanted {
                return;
            }
            let fetch_id = id.clone();
            let result = cx.background_executor().spawn(async move { client.info(&fetch_id) }).await;
            this.update(cx, |this, cx| {
                let state = match result {
                    Ok(info) => {
                        // Every tag we see makes `id:` chips resolve instantly later.
                        for tag in &info.tags {
                            this.tag_names.insert(tag.id, Some((tag.name.clone(), tag.category.clone())));
                        }
                        InfoState::Ready(info)
                    }
                    Err(e) => InfoState::Failed(format!("{e:#}")),
                };
                this.info.insert(id, state);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Put `query` in the search box and run it (used by tags and uploaders).
    fn search_for(&mut self, query: String, window: &mut Window, cx: &mut Context<Self>) {
        // Tag / like / user searches become a named pill, not raw text.
        self.scope = smartq::Scope::parse(&query);
        let text = if self.scope.is_some() { String::new() } else { query };
        self.query.update(cx, |state, cx| state.set_value(text, window, cx));
        self.close_lightbox(cx);
        self.search(true, cx);
    }

    /// Dominant-first colors of the selection, for the adaptive theme.
    fn selected_colors(&mut self) -> Vec<Hsla> {
        match self.selected.clone() {
            Some(Item::Remote(w)) => w.colors.iter().filter_map(|c| parse_hex(c)).collect(),
            Some(Item::Local(path)) => {
                if let Some(colors) = self.local_palettes.get(&path) {
                    return colors.clone();
                }
                // Computed from the small thumbnail once it exists.
                let Some(thumb) = self.local_thumb(&path) else {
                    return Vec::new();
                };
                let colors = adaptive::image_palette(&thumb);
                self.local_palettes.insert(path, colors.clone());
                colors
            }
            None => Vec::new(),
        }
    }

    fn current_items(&self) -> Vec<Item> {
        let mut items: Vec<Item> = match self.tab {
            Tab::Discover => self.results.iter().cloned().map(Item::Remote).collect(),
            Tab::Library => self.library.iter().cloned().map(Item::Local).collect(),
            Tab::Galleries => self.gallery_items(),
            Tab::History => Vec::new(),
        };
        if self.cfg.rainbow {
            // Stable, so equal hues keep their search order.
            items.sort_by(|a, b| {
                let ka = gridmodes::hue_key(&gridmodes::item_colors(a, &self.local_palettes));
                let kb = gridmodes::hue_key(&gridmodes::item_colors(b, &self.local_palettes));
                ka.partial_cmp(&kb).unwrap_or(std::cmp::Ordering::Equal)
            });
        }
        items
    }

    /// Width over height of an item's full image.
    fn item_aspect(&mut self, item: &Item) -> f32 {
        let dims = match item {
            Item::Remote(w) => {
                w.resolution.split_once('x').and_then(|(a, b)| Some((a.parse::<u32>().ok()?, b.parse::<u32>().ok()?)))
            }
            Item::Local(path) => {
                if !self.local_dims.contains_key(path) {
                    if let Ok(d) = image::image_dimensions(path) {
                        self.local_dims.insert(path.clone(), d);
                    }
                }
                self.local_dims.get(path).copied()
            }
        };
        dims.map(|(w, h)| w as f32 / h.max(1) as f32).unwrap_or(1.6)
    }

    fn selected_index(&self, items: &[Item]) -> Option<usize> {
        let selected = self.selected.as_ref()?;
        items.iter().position(|i| same_item(i, selected))
    }

    fn thumb(&mut self, key: String, job: impl FnOnce(String) -> Job) -> Option<PathBuf> {
        if let Some(p) = self.thumbs.get(&key) {
            return Some(p.clone());
        }
        if self.requested.insert(key.clone()) {
            self.pool.request(job(key));
        }
        None
    }

    fn remote_thumb(&mut self, w: &Wallpaper, large: bool) -> Option<PathBuf> {
        let (key, url) = if large {
            (format!("who-{}", w.id), w.thumbs.original.clone())
        } else {
            (format!("wh-{}", w.id), w.thumbs.small.clone())
        };
        self.thumb(key, |key| Job::Remote { key, url })
    }

    /// The full-resolution original, downloaded to the cache on demand.
    fn remote_full(&mut self, w: &Wallpaper, cx: &mut Context<Self>) -> Option<PathBuf> {
        let saved = self.cfg.download_dir.join(w.file_name());
        if saved.is_file() {
            return Some(saved);
        }
        match self.full.get(&w.id) {
            Some(FullState::Ready(path)) => return Some(path.clone()),
            Some(_) => return None,
            None => {}
        }
        let cached = remote_cache_path(&full_key(w), &w.path);
        if cached.is_file() {
            self.full.insert(w.id.clone(), FullState::Ready(cached.clone()));
            return Some(cached);
        }
        self.start_full(w.clone(), SELECTED_WORKERS, cx);
        None
    }

    /// Fetch the original on its own lane (not behind queued thumbnails),
    /// over parallel range requests.
    fn start_full(&mut self, w: Wallpaper, workers: usize, cx: &mut Context<Self>) {
        let progress = Arc::new(Progress::default());
        self.full.insert(w.id.clone(), FullState::Loading { progress: progress.clone() });
        let id = w.id.clone();

        // Repaint the progress readout while the download runs.
        let ticker_progress = progress.clone();
        let ticker_id = id.clone();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(std::time::Duration::from_millis(150)).await;
                let running = this
                    .update(cx, |this, cx| {
                        let running = matches!(
                            this.full.get(&ticker_id),
                            Some(FullState::Loading { progress, .. }) if Arc::ptr_eq(progress, &ticker_progress)
                        );
                        if running {
                            cx.notify();
                        }
                        running
                    })
                    .unwrap_or(false);
                if !running {
                    break;
                }
            }
        })
        .detach();

        let client = self.client.clone();
        let dest = remote_cache_path(&full_key(&w), &w.path);
        let task_progress = progress.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { client.download_parallel_with(&w.path, &dest, &task_progress, workers) })
                .await;
            this.update(cx, |this, cx| {
                // Ignore results of downloads that were cancelled or replaced.
                let current = matches!(
                    this.full.get(&id),
                    Some(FullState::Loading { progress: p, .. }) if Arc::ptr_eq(p, &progress)
                );
                if current {
                    let state = match result {
                        Ok(path) => {
                            this.prewarm_fits(&path);
                            FullState::Ready(path)
                        }
                        Err(e) => FullState::Failed(format!("{e:#}")),
                    };
                    this.full.insert(id, state);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn full_phase(&self, item: &Item) -> FullPhase {
        let Item::Remote(w) = item else {
            return FullPhase::Done;
        };
        match self.full.get(&w.id) {
            None | Some(FullState::Ready(_)) => FullPhase::Done,
            Some(FullState::Failed(e)) => FullPhase::Failed(e.clone()),
            Some(FullState::Loading { progress, .. }) => {
                let done = progress.done.load(Ordering::Relaxed);
                let total = progress.total.load(Ordering::Relaxed);
                FullPhase::Loading(Bands {
                    fraction: (total > 0).then(|| done as f32 / total as f32),
                    mask: progress.chunks.load(Ordering::Relaxed),
                    count: progress.chunk_count.load(Ordering::Relaxed),
                })
            }
        }
    }

    fn retry_full(&mut self, cx: &mut Context<Self>) {
        if let Some(Item::Remote(w)) = &self.selected {
            if matches!(self.full.get(&w.id), Some(FullState::Failed(_))) {
                self.full.remove(&w.id);
                cx.notify();
            }
        }
    }

    /// Queue the preview- and screen-sized copies so neither the inspector
    /// nor the lightbox ever waits on a resize.
    fn prewarm_fits(&mut self, src: &Path) {
        let preview_h =
            image::image_dimensions(src).map(|(w, h)| preview_h_for(w as f32 / h.max(1) as f32)).unwrap_or(PREVIEW_H);
        let w = ((INSPECTOR_W - INSPECTOR_PAD * 2.) * self.scale).round() as u32;
        let h = (preview_h * self.scale).round() as u32;
        self.fitted(src, w, h);
        let bucket = |v: f32| ((v * self.scale / 256.).ceil() * 256.) as u32;
        let (lw, lh) = (bucket(self.viewport.0 - 168.), bucket(self.viewport.1 - 112.));
        self.fitted(src, lw, lh);
    }

    /// Preview box height for the current selection.
    fn preview_h(&self) -> f32 {
        self.selected_dims.map(|(w, h)| preview_h_for(w as f32 / h.max(1) as f32)).unwrap_or(PREVIEW_H)
    }

    /// High-quality copy of `src` fitting `w`x`h` physical pixels.
    fn fitted(&mut self, src: &Path, w: u32, h: u32) -> Option<PathBuf> {
        let path = src.to_owned();
        self.thumb(format!("fit-{w}x{h}-{}", src.display()), |key| Job::Fit { key, path, w, h })
    }

    fn local_thumb(&mut self, path: &Path) -> Option<PathBuf> {
        let path = path.to_owned();
        self.thumb(local_key(&path), |key| Job::Local { key, path })
    }

    fn refresh_library(&mut self) {
        let mut files: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(&self.cfg.download_dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file() && is_image(p))
            .map(|p| (p.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH), p))
            .collect();
        files.sort_by(|a, b| b.0.cmp(&a.0));
        let mut seen: HashSet<PathBuf> = HashSet::new();
        self.library = files
            .into_iter()
            .map(|(_, p)| p)
            .chain(self.applied.desktop.values().cloned())
            .chain(self.applied.lock.values().cloned())
            .filter(|p| p.is_file() && !p.to_string_lossy().contains("-span-"))
            .filter(|p| seen.insert(p.clone()))
            .collect();
    }

    /// What the sidebar, the search box and any scope add up to: the typed
    /// text, the token to remember, and the first page's parameters.
    fn current_search(&self, cx: &App) -> (String, String, SearchParams) {
        // Smart query: recognized words layer filters over the sidebar's,
        // for this search only.
        let typed = self.query.read(cx).value().to_string();
        let parsed = smartq::parse(&typed);
        let ov = parsed.overlay;
        // A scope searches on its own; its token is what gets remembered.
        let (raw, text) = match &self.scope {
            Some(scope) => (scope.token(), scope.token()),
            None => (typed.clone(), parsed.text),
        };
        let mut purity = ov.purity.unwrap_or(self.purity);
        if !self.client.has_key() {
            purity[2] = false;
        }
        if !purity.iter().any(|p| *p) {
            purity = self.purity;
        }
        let params = SearchParams {
            query: text,
            categories: ov.categories.unwrap_or(self.categories),
            purity,
            sorting: ov.sorting.unwrap_or(self.sorting),
            top_range: self.top_range.clone(),
            ratios: ov.ratios.or_else(|| self.ratios.clone()),
            atleast: ov.atleast.or_else(|| self.atleast.clone()),
            resolutions: self.resolutions.clone(),
            color: ov.color.or_else(|| self.color.clone()),
            page: 1,
            seed: None,
        };
        (typed, raw, params)
    }

    fn search(&mut self, reset: bool, cx: &mut Context<Self>) {
        if !reset && (self.searching || self.page >= self.last_page) {
            return;
        }
        if reset {
            self.persist_filters();
        }
        self.search_gen += 1;
        let generation = self.search_gen;
        let (typed, raw, mut params) = self.current_search(cx);
        self.last_query = typed;
        params.page = if reset { 1 } else { self.page + 1 };
        params.seed = if reset { None } else { self.seed.clone() };
        self.searching = true;
        self.tab = Tab::Discover;
        if reset {
            self.status = "Searching...".into();
            self.status_error = false;
        }

        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { client.search(&params) }).await;
            this.update(cx, |this, cx| {
                if this.search_gen != generation {
                    return;
                }
                this.searching = false;
                match result {
                    Ok(resp) => {
                        if reset {
                            this.remember_search(&raw, &resp.data);
                            this.results = resp.data;
                            this.grid_scroll.scroll_to_item(0, ScrollStrategy::Top);
                        } else {
                            this.results.extend(resp.data);
                        }
                        this.page = resp.meta.current_page.max(1);
                        this.last_page = resp.meta.last_page.max(1);
                        this.total = resp.meta.total;
                        if resp.meta.seed.is_some() {
                            this.seed = resp.meta.seed;
                        }
                        this.status = format!("{} wallpapers", this.total).into();
                        this.status_error = false;
                    }
                    Err(e) => {
                        this.set_status(format!("Search failed: {e:#}"), true, cx);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Search as you type: fire once typing pauses.
    fn search_soon(&mut self, cx: &mut Context<Self>) {
        self.type_gen += 1;
        let generation = self.type_gen;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(std::time::Duration::from_millis(380)).await;
            this.update(cx, |this, cx| {
                let now = this.query.read(cx).value().to_string();
                if this.type_gen == generation && now != this.last_query {
                    this.search(true, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Keep the last few searches, with a peek at their results.
    fn remember_search(&mut self, raw: &str, results: &[Wallpaper]) {
        let text = raw.trim();
        if text.is_empty() {
            return;
        }
        self.cfg.recent.retain(|r| r.text != text);
        self.cfg.recent.insert(
            0,
            crate::config::RecentQuery {
                text: text.to_owned(),
                thumbs: results.iter().take(5).map(|w| (w.id.clone(), w.thumbs.small.clone())).collect(),
            },
        );
        self.cfg.recent.truncate(10);
        if let Err(e) = self.cfg.save() {
            eprintln!("saving recent searches: {e:#}");
        }
    }

    /// Make sure the item exists in the library (reusing the cached preview
    /// original, otherwise downloading it).
    fn fetch(client: &Client, dir: &Path, item: Item) -> anyhow::Result<PathBuf> {
        match item {
            Item::Remote(w) => {
                let dest = dir.join(w.file_name());
                let cached = remote_cache_path(&full_key(&w), &w.path);
                if !dest.exists() && cached.is_file() {
                    std::fs::create_dir_all(dir)?;
                    std::fs::copy(&cached, &dest)?;
                    return Ok(dest);
                }
                client.download_parallel(&w.path, &dest, &Progress::default())
            }
            Item::Local(p) => Ok(p),
        }
    }

    fn download_selected(&mut self, cx: &mut Context<Self>) {
        let Some(item) = self.selected.clone() else {
            return;
        };
        let client = self.client.clone();
        let dir = self.cfg.download_dir.clone();
        self.busy += 1;
        self.set_status("Downloading...", false, cx);
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { Self::fetch(&client, &dir, item) }).await;
            this.update(cx, |this, cx| {
                this.busy -= 1;
                match result {
                    Ok(p) => this.set_status(format!("Saved {}", p.display()), false, cx),
                    Err(e) => this.set_status(format!("Download failed: {e:#}"), true, cx),
                }
                this.refresh_library();
            })
            .ok();
        })
        .detach();
    }

    fn assign(&mut self, targets: Vec<Target>, placement: Placement, cx: &mut Context<Self>) {
        let Some(item) = self.selected.clone() else {
            self.set_status("Pick a wallpaper first", true, cx);
            return;
        };
        self.assign_item(item, targets, placement, cx);
    }

    fn assign_item(&mut self, item: Item, targets: Vec<Target>, placement: Placement, cx: &mut Context<Self>) {
        self.assign_item_then(item, targets, placement, false, cx);
    }

    /// Stage `item`; with `then_apply`, also put it up as soon as it's ready.
    fn assign_item_then(
        &mut self,
        item: Item,
        targets: Vec<Target>,
        placement: Placement,
        then_apply: bool,
        cx: &mut Context<Self>,
    ) {
        let client = self.client.clone();
        let dir = self.cfg.download_dir.clone();
        let monitors = self.monitors.clone();
        self.busy += 1;
        let msg = match placement {
            Placement::Span => "Downloading and slicing across displays...",
            _ => "Downloading...",
        };
        let landed = match &placement {
            Placement::Monitor(name) => Some(name.clone()),
            _ => None,
        };
        self.set_status(msg, false, cx);

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let path = Self::fetch(&client, &dir, item)?;
                    let map: BTreeMap<String, PathBuf> = match placement {
                        Placement::Monitor(name) => [(name, path)].into(),
                        Placement::All => monitors.iter().map(|m| (m.name.clone(), path.clone())).collect(),
                        Placement::Span => span::slice(&path, &monitors, &cache_dir().join("span"))?,
                    };
                    anyhow::Ok(map)
                })
                .await;
            this.update(cx, |this, cx| {
                this.busy -= 1;
                if let Some(name) = &landed {
                    this.landing.retain(|(m, _)| m != name);
                }
                match result {
                    Ok(map) => {
                        for t in &targets {
                            match t {
                                Target::Desktop => this.desktop.extend(map.clone()),
                                Target::Lock => this.lock.extend(map.clone()),
                            }
                        }
                        this.refresh_library();
                        if then_apply {
                            this.apply(targets.clone(), cx);
                        } else {
                            this.set_status("Staged. Press Apply to set it.", false, cx);
                        }
                    }
                    Err(e) => this.set_status(format!("Failed: {e:#}"), true, cx),
                }
            })
            .ok();
        })
        .detach();
    }

    fn apply(&mut self, targets: Vec<Target>, cx: &mut Context<Self>) {
        let monitors = self.monitors.clone();
        let lock_mode = self.cfg.lock_mode;
        let backend = self.backend.clone();
        let jobs: Vec<(Target, BTreeMap<String, PathBuf>)> = targets
            .into_iter()
            .map(|t| {
                let map = match t {
                    Target::Desktop => self.desktop.clone(),
                    Target::Lock => self.lock.clone(),
                };
                (t, map)
            })
            .filter(|(_, m)| !m.is_empty())
            .collect();
        if jobs.is_empty() {
            self.set_status("Nothing staged yet: click a display to put the wallpaper on it", true, cx);
            return;
        }
        self.busy += 1;
        self.set_status("Applying...", false, cx);

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let mut done = Vec::new();
                    for (target, images) in jobs {
                        match target {
                            Target::Desktop => {
                                backend.desktop().context("no desktop backend")?.set(&images, &monitors)?
                            }
                            Target::Lock => {
                                backend.lock().context("no lock screen")?.apply(&images, &monitors, lock_mode)?
                            }
                        }
                        done.push((target, images));
                    }
                    anyhow::Ok(done)
                })
                .await;
            this.update(cx, |this, cx| {
                this.busy -= 1;
                match result {
                    Ok(done) => {
                        let names: Vec<&str> = done.iter().map(|(t, _)| t.label()).collect();
                        let msg = format!("{} updated", names.join(" and "));
                        for (target, images) in done {
                            match target {
                                Target::Desktop => {
                                    this.applied.record("desktop", &images);
                                    this.applied.desktop = images;
                                }
                                Target::Lock => {
                                    this.applied.record("lock", &images);
                                    this.applied.lock = images;
                                }
                            }
                        }
                        if let Err(e) = this.applied.save() {
                            eprintln!("saving state: {e:#}");
                        }
                        this.set_status(msg, false, cx);
                    }
                    Err(e) => this.set_status(format!("Apply failed: {e:#}"), true, cx),
                }
            })
            .ok();
        })
        .detach();
    }

    fn set_lock_mode(&mut self, mode: LockMode, cx: &mut Context<Self>) {
        self.cfg.lock_mode = mode;
        if let Err(e) = self.cfg.save() {
            self.set_status(format!("Saving config failed: {e:#}"), true, cx);
        }
        cx.notify();
    }

    fn set_min_resolution(&mut self, res: Option<&str>, cx: &mut Context<Self>) {
        self.atleast = res.map(str::to_owned);
        self.resolutions = None;
        self.search(true, cx);
    }

    // ---- sidebar ------------------------------------------------------------

    /// The tab switcher: four live tiles, each a strip of what the tab holds.
    /// Inactive ones rest in grayscale and bloom into color on hover; the
    /// active one is in color, edged in the accent, and a light sweeps across
    /// it when it's chosen.
    fn render_nav(&mut self, cx: &mut Context<Self>) -> Div {
        let p = pal();
        let tab = self.tab;

        // A few thumbnails per tab.
        let results: Vec<Wallpaper> = self.results.iter().take(3).cloned().collect();
        let discover: Vec<PathBuf> = results.iter().filter_map(|w| self.remote_thumb(w, false)).collect();
        let latest: Vec<PathBuf> = self.library.iter().take(3).cloned().collect();
        let library: Vec<PathBuf> = latest.iter().filter_map(|path| self.local_thumb(path)).collect();
        let covers: Vec<gallery_entry::Entry> =
            self.galleries.list.iter().filter_map(|g| g.items.first().cloned()).take(3).collect();
        let galleries: Vec<PathBuf> = covers
            .iter()
            .filter_map(|e| match e {
                gallery_entry::Entry::Wallhaven(w) => self.remote_thumb(w, false),
                gallery_entry::Entry::Local(path) => self.local_thumb(path),
            })
            .collect();
        let last_set: Vec<PathBuf> = self
            .applied
            .history
            .iter()
            .rev()
            .find(|e| e.target == "desktop")
            .map(|e| self.monitors.iter().filter_map(|m| e.images.get(&m.name).cloned()).take(3).collect())
            .unwrap_or_default();
        let history: Vec<PathBuf> = last_set.iter().filter_map(|path| self.local_thumb(path)).collect();

        // Each tile glows in its own images' colors, like the cards do.
        let tint_of = |items: Vec<Item>, palettes: &HashMap<PathBuf, Vec<Hsla>>| -> Hsla {
            let colors: Vec<Hsla> = items.iter().flat_map(|i| gridmodes::item_colors(i, palettes)).collect();
            glass::glow_color(&colors).unwrap_or(p.accent)
        };
        let tints = [
            tint_of(results.iter().cloned().map(Item::Remote).collect(), &self.local_palettes),
            tint_of(latest.iter().cloned().map(Item::Local).collect(), &self.local_palettes),
            tint_of(covers.iter().map(galleries::item_of).collect(), &self.local_palettes),
            tint_of(last_set.iter().cloned().map(Item::Local).collect(), &self.local_palettes),
        ];

        let counts = [
            (self.total > 0).then(|| compact_count(self.total)),
            Some(compact_count(self.library.len() as u64)),
            Some(compact_count(self.galleries.list.len() as u64)),
            Some(compact_count(self.applied.history.len() as u64)),
        ];

        let tile = |ix: usize,
                    id: &'static str,
                    icon_name: &'static str,
                    label: &'static str,
                    t: Tab,
                    thumbs: Vec<PathBuf>| {
            let on = tab == t;
            let c = tints[ix];
            // Readable on the dark wash at the tile's foot.
            let label_c = adaptive::readable(Hsla { l: c.l.max(0.62), ..c }, hsla(0., 0., 0.06, 1.), 4.5);
            let group = SharedString::from(format!("{id}-g"));
            let strip = |gray: bool| {
                h_flex().absolute().inset_0().gap(px(1.)).children(thumbs.iter().map(|t| {
                    div()
                        .flex_1()
                        .h_full()
                        .child(img(t.clone()).size_full().object_fit(ObjectFit::Cover).grayscale(gray))
                }))
            };
            let empty = thumbs.is_empty();
            div()
                .id(id)
                .group(group.clone())
                .relative()
                .h(px(54.))
                .rounded_lg()
                .overflow_hidden()
                .cursor_pointer()
                .bg(linear_gradient(
                    135.,
                    linear_color_stop(with_alpha(c, if on { 0.35 } else { 0.12 }), 0.),
                    linear_color_stop(p.surface2, 1.),
                ))
                .border_1()
                .border_color(if on { c } else { p.border })
                .when(!on, |d| d.hover(move |s| s.border_color(with_alpha(c, 0.6))))
                .when(empty, |d| {
                    d.child(
                        div()
                            .absolute()
                            .right(px(-6.))
                            .top(px(-6.))
                            .child(icon(icon_name).size(px(44.)).text_color(with_alpha(p.fg, 0.06))),
                    )
                })
                // Resting: grayscale under a dark wash. Hover or active: color.
                .when(!empty, |d| {
                    if on {
                        d.child(strip(false))
                    } else {
                        d.child(strip(true)).child(strip(false).invisible().group_hover(group.clone(), |s| s.visible()))
                    }
                })
                .child(div().absolute().inset_0().bg(linear_gradient(
                    0.,
                    linear_color_stop(hsla(0., 0., 0., if on { 0.72 } else { 0.82 }), 0.),
                    linear_color_stop(hsla(0., 0., 0., if on { 0.05 } else { 0.4 }), 1.),
                )))
                .when(on, |d| {
                    // One pass of light across the chosen tile.
                    d.child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .w(px(40.))
                            .bg(linear_gradient(
                                90.,
                                linear_color_stop(hsla(0., 0., 1., 0.), 0.),
                                linear_color_stop(hsla(0., 0., 1., 0.0), 1.),
                            ))
                            .with_animation(
                                SharedString::from(format!("sweep-{id}")),
                                Animation::new(std::time::Duration::from_millis(700)).with_easing(ease_in_out),
                                move |d, t| {
                                    // The sweep is the tile's own color, lifted.
                                    let a = (1. - (2. * t - 1.).abs()) * 0.3;
                                    let light = Hsla { l: 0.8, a, ..c };
                                    d.left(px(-40. + 200. * t)).bg(linear_gradient(
                                        90.,
                                        linear_color_stop(with_alpha(light, 0.), 0.),
                                        linear_color_stop(light, 0.5),
                                    ))
                                },
                            ),
                    )
                })
                .child(
                    h_flex()
                        .absolute()
                        .left(px(9.))
                        .right(px(9.))
                        .bottom(px(6.))
                        .gap_1p5()
                        .child(icon(icon_name).size_3p5().text_color(if on { label_c } else { hsla(0., 0., 1., 0.7) }))
                        .child(
                            div()
                                .flex_1()
                                .text_sm()
                                .font_weight(if on { FontWeight::SEMIBOLD } else { FontWeight::MEDIUM })
                                .text_color(if on { white() } else { hsla(0., 0., 1., 0.78) })
                                .child(label),
                        )
                        .children(counts[ix].clone().map(|c| {
                            div().text_xs().text_color(hsla(0., 0., 1., if on { 0.75 } else { 0.5 })).child(c)
                        })),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    match t {
                        Tab::Library => this.refresh_library(),
                        Tab::Galleries => return this.show_galleries(cx),
                        _ => {}
                    }
                    this.tab = t;
                    cx.notify();
                }))
        };

        div()
            .grid()
            .grid_cols(2)
            .gap_1p5()
            .child(tile(0, "nav-discover", "compass", "Discover", Tab::Discover, discover))
            .child(tile(1, "nav-library", "folder", "Library", Tab::Library, library))
            .child(tile(2, "nav-galleries", "images", "Galleries", Tab::Galleries, galleries))
            .child(tile(3, "nav-history", "calendar", "History", Tab::History, history))
    }

    fn render_sidebar(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = pal();
        let tab = self.tab;
        let has_key = self.client.has_key();

        let nav = self.render_nav(cx);

        let mut cards = v_flex().gap_3().px_3().py_3();
        if tab == Tab::Discover {
            // Sort.
            let sorts = Sorting::ALL.iter().enumerate().map(|(i, &s)| {
                opt(("sort", i), s.label(), self.sorting == s, None, p).on_click(cx.listener(move |this, _, _, cx| {
                    this.sorting = s;
                    this.search(true, cx);
                }))
            });
            let sort_summary = if self.sorting == Sorting::Toplist {
                format!("Top {}", self.top_range)
            } else {
                self.sorting.label().to_owned()
            };
            let ranges = (self.sorting == Sorting::Toplist).then(|| {
                let ranges = TOP_RANGES.iter().enumerate().map(|(i, &r)| {
                    segment(("range", i), r, self.top_range == r, p).px_0().on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.top_range = r.into();
                            this.search(true, cx);
                        },
                    ))
                });
                segmented_track(p).children(ranges)
            });
            cards = cards.child(
                side_card("arrow-down-wide-narrow", "Sort", Some(sort_summary.into()), p)
                    .child(opt_grid(4).children(sorts))
                    .children(ranges),
            );

            // Shape: aspect ratio and minimum resolution.
            let mut ratios: Vec<(String, Option<String>)> =
                RATIO_PRESETS.iter().map(|(l, v)| (l.to_string(), v.map(str::to_owned))).collect();
            if let Some(acc) = self.account.as_ref().and_then(|a| a.ratios()) {
                if !ratios.iter().any(|(_, v)| v.as_deref() == Some(&acc)) {
                    ratios.push(("Account".into(), Some(acc)));
                }
            }
            let ratio_label = ratios.iter().find(|(_, v)| *v == self.ratios).map(|(l, _)| l.clone());
            let ratio_cols = ratios.len() as u16;
            let ratio_opts = ratios.into_iter().enumerate().map(|(i, (label, value))| {
                let on = self.ratios == value;
                opt(("ratio", i), label, on, None, p).on_click(cx.listener(move |this, _, _, cx| {
                    this.ratios = value.clone();
                    this.search(true, cx);
                }))
            });
            let res_label = if self.resolutions.is_some() {
                Some("Account".to_owned())
            } else {
                RES_PRESETS.iter().find(|(_, v)| *v == self.atleast.as_deref()).map(|(l, _)| l.to_string())
            };
            let res_opts = RES_PRESETS.iter().enumerate().map(|(i, &(label, value))| {
                let on = self.resolutions.is_none() && self.atleast.as_deref() == value;
                opt(("res", i), label, on, None, p)
                    .on_click(cx.listener(move |this, _, _, cx| this.set_min_resolution(value, cx)))
            });
            let res_account = self.account.as_ref().and_then(|a| a.resolutions()).map(|res| {
                let on = self.resolutions.as_deref() == Some(&res);
                opt("res-account", "Account resolutions", on, None, p).on_click(cx.listener(move |this, _, _, cx| {
                    this.resolutions = Some(res.clone());
                    this.atleast = None;
                    this.search(true, cx);
                }))
            });
            let shape_summary = match (ratio_label.as_deref(), res_label.as_deref()) {
                (Some("Any") | None, Some("Any") | None) => "Any".to_owned(),
                (Some("Any") | None, Some(r)) => format!("{r}+"),
                (Some(a), Some("Any") | None) => a.to_owned(),
                (Some(a), Some(r)) => format!("{a}, {r}+"),
            };
            cards = cards.child(
                side_card("proportions", "Shape", Some(shape_summary.into()), p)
                    .child(sub_label("Aspect ratio", p).child(opt_grid(ratio_cols).children(ratio_opts)))
                    .child(sub_label("At least", p).child(opt_grid(4).children(res_opts)).children(res_account)),
            );

            // Content: categories and purity.
            let cats = CATEGORIES.iter().enumerate().map(|(i, &label)| {
                opt(("cat", i), label, self.categories[i], None, p).on_click(cx.listener(move |this, _, _, cx| {
                    this.categories[i] = !this.categories[i];
                    if !this.categories.iter().any(|b| *b) {
                        this.categories[i] = true;
                    }
                    this.search(true, cx);
                }))
            });
            // NSFW needs an API key, so it isn't offered without one.
            let purities: Vec<_> = PURITIES
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != 2 || has_key)
                .map(|(i, &label)| {
                    opt(("purity", i), label, self.purity[i], Some(purity_color(label)), p).on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.purity[i] = !this.purity[i];
                            if !this.purity.iter().any(|b| *b) {
                                this.purity[i] = true;
                            }
                            this.search(true, cx);
                        },
                    ))
                })
                .collect();
            let purity_cols = purities.len() as u16;
            // The header shows which purities are on, as their colored dots.
            let purity_dots = h_flex().gap_1().children(
                PURITIES
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| self.purity[*i])
                    .map(|(_, &label)| div().size(px(7.)).rounded_full().bg(purity_color(label))),
            );
            let on_cats: Vec<&str> =
                CATEGORIES.iter().enumerate().filter(|(i, _)| self.categories[*i]).map(|(_, &c)| c).collect();
            let cat_summary = if on_cats.len() == CATEGORIES.len() { "All".to_owned() } else { on_cats.join(", ") };
            cards = cards.child(
                side_card_with("layers", "Content", p, |h| {
                    h.child(div().text_xs().text_color(p.dim).truncate().child(cat_summary)).child(purity_dots)
                })
                .child(opt_grid(3).children(cats))
                .child(opt_grid(purity_cols).children(purities)),
            );

            // Color.
            let current_color = self.color.clone();
            let any_swatch = div()
                .id("color-any")
                .h(px(20.))
                .rounded_md()
                .flex()
                .items_center()
                .justify_center()
                .bg(p.bg)
                .border_1()
                .border_color(if current_color.is_none() { p.fg } else { p.border })
                .cursor_pointer()
                .hover(move |s| s.border_color(p.border_strong))
                .child(icon("ban").size_3().text_color(p.muted))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.color = None;
                    this.search(true, cx);
                }));
            let swatches = WALLHAVEN_COLORS.iter().enumerate().map(|(i, &c)| {
                let on = current_color.as_deref() == Some(c);
                div()
                    .id(("color", i))
                    .h(px(20.))
                    .rounded_md()
                    .bg(hex(u32::from_str_radix(c, 16).unwrap_or(0)))
                    .border_2()
                    .border_color(if on { p.fg } else { with_alpha(p.border, 0.) })
                    .cursor_pointer()
                    .hover(move |s| s.border_color(with_alpha(p.fg, 0.5)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.color = Some(c.to_owned());
                        this.search(true, cx);
                    }))
            });
            let color_head = current_color.as_deref().and_then(|c| u32::from_str_radix(c, 16).ok());
            cards = cards.child(
                side_card_with("palette", "Color", p, |h| match color_head {
                    Some(c) => h.child(
                        div().w(px(18.)).h(px(10.)).rounded_sm().bg(hex(c)).border_1().border_color(p.border_strong),
                    ),
                    None => h.child(div().text_xs().text_color(p.dim).child("Any")),
                })
                .child(div().grid().grid_cols(8).gap_1().child(any_swatch).children(swatches)),
            );
        } else if tab == Tab::Galleries {
            cards = cards.child(self.render_gallery_side(cx));
        } else if tab == Tab::Library {
            cards = cards.child(
                side_card("folder", "Library", Some(compact_count(self.library.len() as u64).into()), p).child(
                    div()
                        .text_xs()
                        .text_color(p.muted)
                        .child("Wallpapers you have downloaded or set. Select one to put it on a display."),
                ),
            );
        } else {
            cards = cards.child(
                side_card("calendar", "History", Some(compact_count(self.applied.history.len() as u64).into()), p)
                    .child(div().text_xs().text_color(p.muted).child(
                        "Every set you've applied. Restore or re-apply one, or hover it and hold Space to see it on \
                         your desktop.",
                    )),
            );
        }

        let saved = self.cfg.filters.count();
        let defaults_name = if self.account.is_some() { "account defaults" } else { "defaults" };

        let backdrop = self.backdrop();
        v_flex()
            .relative()
            .overflow_hidden()
            .w(px(SIDEBAR_W))
            .h_full()
            .flex_none()
            .bg(p.surface)
            .border_r_1()
            .border_color(p.border)
            .children(glass::glass_layers(backdrop, "side", p))
            .child(
                title_region(h_flex(), p)
                    .gap_2p5()
                    .px_5()
                    // Room for the system window buttons.
                    .when(NATIVE_WINDOW_BUTTONS, |d| d.pl(px(84.)))
                    .child(
                        // The logo's gradient slowly turns, in the live palette.
                        div().size(px(26.)).rounded_lg().with_animation(
                            "logo-spin",
                            Animation::new(std::time::Duration::from_secs(8)).repeat(),
                            move |d, t| {
                                d.bg(linear_gradient(
                                    135. + t * 360.,
                                    linear_color_stop(p.accent, 0.),
                                    linear_color_stop(hex(0x22d3ee), 1.),
                                ))
                            },
                        ),
                    )
                    .child(div().text_base().font_weight(FontWeight::BOLD).child("muralist"))
                    .child(div().flex_1())
                    .child(
                        no_drag(icon_button("open-settings", "settings", p))
                            .on_click(cx.listener(|this, _, window, cx| this.open_settings(window, cx))),
                    ),
            )
            .child(div().px_3().pt_3().child(nav))
            .child(div().id("filters").flex_1().min_h_0().overflow_y_scroll().child(cards))
            .children(self.render_drop_dock(cx))
            .when(!has_key, |d| {
                d.child(
                    h_flex()
                        .id("add-key")
                        .mx_3()
                        .mb_3()
                        .gap_2p5()
                        .p_3()
                        .rounded_xl()
                        .bg(with_alpha(p.accent, 0.1))
                        .border_1()
                        .border_color(with_alpha(p.accent, 0.35))
                        .text_xs()
                        .cursor_pointer()
                        .hover(move |s| s.bg(with_alpha(p.accent, 0.18)))
                        .child(icon("key-round").size_4().text_color(p.accent))
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .child(div().font_weight(FontWeight::MEDIUM).child("Add an API key"))
                                .child(div().text_color(p.dim).child("Unlocks NSFW and your defaults")),
                        )
                        .on_click(cx.listener(|this, _, window, cx| this.open_settings(window, cx))),
                )
            })
            .when(tab == Tab::Discover, |d| {
                d.child(
                    h_flex().h(px(48.)).flex_none().px_4().gap_2().border_t_1().border_color(p.border).text_xs().map(
                        |d| {
                            if saved == 0 {
                                d.text_color(p.dim)
                                    .child(icon("check").size_3p5())
                                    .child(format!("Using your {defaults_name}"))
                            } else {
                                d.child(div().size(px(6.)).rounded_full().bg(p.accent))
                                    .child(
                                        div().flex_1().text_color(p.muted).child(format!(
                                            "{saved} saved filter{}",
                                            if saved == 1 { "" } else { "s" }
                                        )),
                                    )
                                    .child(
                                        kit::Btn::new("reset")
                                            .label("Reset")
                                            .icon("rotate-ccw")
                                            .ghost()
                                            .sm()
                                            .on_click(cx.listener(|this, _, _, cx| this.reset_filters(cx))),
                                    )
                            }
                        },
                    ),
                )
            })
    }

    // ---- main column --------------------------------------------------------

    fn render_topbar(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = pal();
        let bar = title_region(h_flex(), p).gap_3().px(px(GRID_PAD));

        match self.tab {
            Tab::Discover => bar
                .child(no_drag(self.render_searchbox(cx)))
                .child(no_drag(
                    segmented_track(p).flex_none().child(
                        kit::Btn::new("save-search")
                            .icon("bookmark-plus")
                            .ghost()
                            .sm()
                            .tooltip("Save this search as a smart gallery")
                            .on_click(cx.listener(|this, _, _, cx| this.save_search_as_gallery(cx))),
                    ),
                ))
                .child(no_drag(self.render_view_toggles(cx))),
            Tab::Galleries => self.render_gallery_bar(bar, cx),
            Tab::Library => {
                let dir = self.cfg.download_dir.clone();
                let tints: Vec<Hsla> = self
                    .library
                    .iter()
                    .take(8)
                    .filter_map(|path| self.local_palettes.get(path).and_then(|c| c.first().copied()))
                    .collect();
                let n = self.library.len();
                let open_dir = {
                    let dir = dir.clone();
                    move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                        cx.open_url(&format!("file://{}", dir.display()))
                    }
                };
                let field = bar_field("library-field", &tints, p)
                    .child(icon("folder").size_4().flex_none().text_color(p.accent))
                    .child(div().flex_none().font_weight(FontWeight::SEMIBOLD).child("Library"))
                    .child(
                        no_drag(path_crumbs("library-path", &dir, p))
                            .tooltip(|window, cx| Tooltip::new("Open folder").build(window, cx))
                            .on_click(open_dir.clone()),
                    )
                    .child(div().flex_1())
                    .child(div().flex_none().text_xs().text_color(p.dim).child(format!(
                        "{} wallpaper{}",
                        thousands(n as u64),
                        if n == 1 { "" } else { "s" }
                    )));
                let actions = segmented_track(p)
                    .flex_none()
                    .child(
                        kit::Btn::new("open-dir")
                            .icon("folder-open")
                            .ghost()
                            .sm()
                            .tooltip("Open folder")
                            .on_click(open_dir),
                    )
                    .child(kit::Btn::new("refresh").icon("refresh-cw").ghost().sm().tooltip("Rescan folder").on_click(
                        cx.listener(|this, _, _, cx| {
                            this.refresh_library();
                            cx.notify();
                        }),
                    ));
                bar.child(no_drag(field)).child(no_drag(actions)).child(no_drag(self.render_view_toggles(cx)))
            }
            Tab::History => {
                let n = self.applied.history.len();
                let tints: Vec<Hsla> = self
                    .applied
                    .history
                    .iter()
                    .rev()
                    .take(8)
                    .filter_map(|e| {
                        e.images.values().next().and_then(|path| self.local_palettes.get(path)).and_then(|c| c.first())
                    })
                    .copied()
                    .collect();
                let since = self
                    .applied
                    .history
                    .first()
                    .and_then(|e| chrono::DateTime::from_timestamp(e.at, 0))
                    .map(|t| format!("since {}", t.with_timezone(&chrono::Local).format("%b %-d")));
                let field = bar_field("history-field", &tints, p)
                    .child(icon("calendar").size_4().flex_none().text_color(p.accent))
                    .child(div().flex_none().font_weight(FontWeight::SEMIBOLD).child("History"))
                    .child(div().min_w_0().truncate().text_sm().text_color(p.muted).child(match (n, since) {
                        (0, _) => "Nothing set yet".to_owned(),
                        (n, Some(since)) => {
                            format!("{} set{} {since}", thousands(n as u64), if n == 1 { "" } else { "s" })
                        }
                        (n, None) => format!("{} set{}", thousands(n as u64), if n == 1 { "" } else { "s" }),
                    }))
                    .child(div().flex_1())
                    .when(n > 0, |d| {
                        d.child(
                            h_flex()
                                .flex_none()
                                .gap_1p5()
                                .text_xs()
                                .text_color(p.dim)
                                .child(keycap("Space", p))
                                .child("hold over one to peek"),
                        )
                    });
                bar.child(no_drag(field)).when(n > 0, |d| {
                    d.child(no_drag(
                        segmented_track(p).flex_none().child(
                            kit::Btn::new("clear-history")
                                .icon("delete")
                                .ghost()
                                .sm()
                                .tooltip("Clear history")
                                .on_click(cx.listener(|this, _, _, cx| this.clear_history(cx))),
                        ),
                    ))
                })
            }
        }
    }

    /// Grid / Mosaic switch plus the Rainbow ordering toggle.
    /// Grid / Mosaic plus Rainbow ordering, as one compact icon group.
    fn render_view_toggles(&mut self, cx: &mut Context<Self>) -> Div {
        let p = pal();
        let layout = self.cfg.layout;
        let rainbow = self.cfg.rainbow;
        let save = |this: &mut Self| {
            if let Err(e) = this.cfg.save() {
                eprintln!("saving config: {e:#}");
            }
        };
        let mode = |id: &'static str, name: &'static str, tip: &'static str, m: GridMode| {
            kit::Btn::new(id)
                .icon(name)
                .ghost()
                .sm()
                .selected(layout == m)
                .tooltip(tip)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.cfg.layout = m;
                    save(this);
                    cx.notify();
                }))
                .render()
        };
        segmented_track(p)
            .flex_none()
            .child(mode("view-grid", "layout-dashboard", "Grid", GridMode::Grid))
            .child(mode("view-mosaic", "gallery-vertical-end", "Mosaic: real shapes", GridMode::Mosaic))
            .child(div().w(px(1.)).h(px(16.)).mx_0p5().bg(p.border_strong))
            .child(
                kit::Btn::new("rainbow")
                    .ghost()
                    .sm()
                    .selected(rainbow)
                    .tooltip(if rainbow { "Rainbow order: on" } else { "Order results by color" })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.cfg.rainbow = !this.cfg.rainbow;
                        save(this);
                        cx.notify();
                    }))
                    .render()
                    .child(div().w(px(14.)).h(px(14.)).rounded_full().bg(linear_gradient(
                        135.,
                        linear_color_stop(hsla(0.0, 0.8, 0.6, 1.), 0.),
                        linear_color_stop(hsla(0.6, 0.8, 0.6, 1.), 1.),
                    ))),
            )
    }

    fn render_card(&mut self, ix: usize, item: Item, w: f32, h: f32, cx: &mut Context<Self>) -> AnyElement {
        let p = pal();
        let selected = match (&self.selected, &item) {
            (Some(Item::Remote(a)), Item::Remote(b)) => a.id == b.id,
            (Some(Item::Local(a)), Item::Local(b)) => a == b,
            _ => false,
        };
        let purity: Option<String> = match &item {
            Item::Remote(wp) => Some(wp.purity.clone()),
            Item::Local(_) => None,
        };
        // Placeholder in the wallpaper's own dominant color, dimmed.
        let placeholder_bg = match &item {
            Item::Remote(wp) => wp
                .colors
                .first()
                .and_then(|c| parse_hex(c))
                .map(|c| Hsla { l: (c.l * 0.45).min(0.3), ..c })
                .unwrap_or(p.surface2),
            Item::Local(_) => p.surface2,
        };
        // Mosaic cards that aren't thumbnail-shaped use Wallhaven's
        // shape-preserving thumb instead of the 3:2 crop.
        let true_shape = self.cfg.layout == GridMode::Mosaic && (w / h - 1.5).abs() > 0.3;
        let card_key = match &item {
            Item::Remote(wp) if true_shape => format!("who-{}", wp.id),
            Item::Remote(wp) => format!("wh-{}", wp.id),
            Item::Local(path) => local_key(path),
        };
        let holo = self.holo.filter(|(hix, _, _)| *hix == ix).map(|(_, rx, ry)| (rx, ry));
        let hovered = holo.is_some();
        // Palette for the hover effects; local images get theirs on first hover.
        let colors = if hovered {
            if let Item::Local(path) = &item {
                if !self.local_palettes.contains_key(path) {
                    if let Some(thumb) = self.local_thumb(path) {
                        self.local_palettes.insert(path.clone(), adaptive::image_palette(&thumb));
                    }
                }
            }
            gridmodes::item_colors(&item, &self.local_palettes)
        } else {
            Vec::new()
        };
        // Labels drift against the pointer for a hint of depth.
        let (dx, dy) = holo.map(|(rx, ry)| ((0.5 - rx) * 8., (0.5 - ry) * 4.)).unwrap_or((0., 0.));
        let fresh =
            self.thumb_arrived.get(&card_key).is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(600));
        let (thumb, left, right) = match &item {
            Item::Remote(wp) => {
                (self.remote_thumb(wp, true_shape), wp.resolution.clone(), Some(wp.favorites.to_string()))
            }
            Item::Local(path) => (self.local_thumb(path), file_name(path), None),
        };
        let drag_thumb = thumb.clone();

        // Spoilers: a pixel mosaic until hovered.
        let spoiler = purity.as_deref().is_some_and(|pu| self.cfg.spoiler.hides(pu)) && !selected;
        let mosaic = match (&thumb, spoiler) {
            (Some(t), true) => {
                let src = t.clone();
                self.thumb(format!("pix-{card_key}"), |key| Job::Pixelate { key, path: src })
            }
            _ => None,
        };
        let hide = spoiler && !(hovered && self.cfg.spoiler_hover);

        let image: AnyElement = match (&thumb, hide) {
            (_, true) => match mosaic {
                Some(m) => radius(img(m).size_full().object_fit(ObjectFit::Cover), CARD_INNER_R).into_any_element(),
                None => shimmer(SharedString::from(format!("card-shimmer-{card_key}"))).into_any_element(),
            },
            (Some(path), false) => {
                let shimmer_id = SharedString::from(format!("card-shimmer-{card_key}"));
                // Keep the placeholder up while the image decodes: no blank frame.
                let image = radius(
                    img(path.clone())
                        .size_full()
                        .object_fit(ObjectFit::Cover)
                        .with_loading(move || shimmer(shimmer_id.clone()).into_any_element()),
                    CARD_INNER_R,
                );
                if spoiler {
                    // Revealed on hover: sharpen in.
                    fade_in(SharedString::from(format!("reveal-{card_key}")), image).into_any_element()
                } else if fresh {
                    fade_in(SharedString::from(format!("card-fade-{card_key}")), image).into_any_element()
                } else {
                    image.into_any_element()
                }
            }
            (None, false) => shimmer(SharedString::from(format!("card-shimmer-{card_key}"))).into_any_element(),
        };

        let card = div()
            .id(("card", ix))
            .relative()
            .w(px(w))
            .h(px(h))
            .flex_none()
            .rounded_xl()
            .overflow_hidden()
            .cursor_pointer()
            .bg(placeholder_bg)
            // Purity reads from the edge itself: thin for SFW, dashed for
            // Sketchy, heavy (and breathing, below) for NSFW.
            .map(|d| match (selected, purity.as_deref()) {
                (true, _) => d.border_2().border_color(p.accent).shadow_lg(),
                (false, Some("sfw")) => d.border_1().border_color(with_alpha(purity_color("sfw"), 0.5)),
                (false, Some("sketchy")) => {
                    d.border_2().border_dashed().border_color(with_alpha(purity_color("sketchy"), 0.85))
                }
                (false, Some(_)) => d.border_2().border_color(with_alpha(purity_color("nsfw"), 0.75)),
                (false, None) => {
                    d.border_2().border_color(with_alpha(p.border, 0.)).hover(move |s| s.border_color(p.border_strong))
                }
            })
            .child(image)
            .when(hide, |d| {
                d.child(
                    v_flex()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .items_center()
                        .justify_center()
                        .gap_1()
                        .text_xs()
                        .text_color(hsla(0., 0., 1., 0.85))
                        .child(icon("eye-off").size_5())
                        .child(if self.cfg.spoiler_hover { "Hover to reveal" } else { "Click to reveal" }),
                )
            })
            .child(radius_bottom(div(), CARD_INNER_R).absolute().bottom_0().left_0().right_0().h(px(64.)).bg(
                linear_gradient(
                    180.,
                    linear_color_stop(hsla(0., 0., 0., 0.), 0.),
                    linear_color_stop(hsla(0., 0., 0., 0.8), 1.),
                ),
            ))
            .child(
                h_flex()
                    .absolute()
                    .bottom(px(8. + dy))
                    .left(px(12. + dx))
                    .right(px(12. - dx))
                    .justify_between()
                    .gap_2()
                    .text_xs()
                    .text_color(white())
                    .child(div().truncate().font_weight(FontWeight::MEDIUM).child(left))
                    .when_some(right, |d, favs| {
                        d.child(
                            h_flex()
                                .gap_1()
                                .flex_none()
                                .text_color(hsla(0., 0., 1., 0.8))
                                .child(icon("heart").size_3())
                                .child(favs),
                        )
                    }),
            )
            // Purity tab hanging from the top edge, like a folder tab.
            .when_some(purity.clone(), |d, purity| {
                let c = purity_color(&purity);
                d.child(
                    h_flex()
                        .absolute()
                        .top_0()
                        .left(relative(0.5))
                        .ml(px(-30.))
                        .w(px(60.))
                        .h(px(16.))
                        .justify_center()
                        .rounded_b_md()
                        .bg(c)
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(on_accent(c))
                        .child(purity_label(&purity)),
                )
            })
            .when_some(holo.filter(|_| !hide).map(|(rx, ry)| (rx, ry, colors.clone())), |d, (rx, ry, colors)| {
                d.children(gridmodes::holo_layers(rx, ry, &colors))
            })
            .on_mouse_move(cx.listener(move |this, ev: &MouseMoveEvent, _, cx| {
                let Some(b) = this.card_bounds.borrow().get(&ix).copied() else {
                    return;
                };
                let rx = f32::from(ev.position.x - b.origin.x) / f32::from(b.size.width).max(1.);
                let ry = f32::from(ev.position.y - b.origin.y) / f32::from(b.size.height).max(1.);
                this.holo = Some((ix, rx.clamp(0., 1.), ry.clamp(0., 1.)));
                cx.notify();
            }))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if !hovered && this.holo.is_some_and(|(h, _, _)| h == ix) {
                    this.holo = None;
                    cx.notify();
                }
            }))
            .on_mouse_down(MouseButton::Right, {
                let item = item.clone();
                cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                    this.picker = Some(galleries::Picker { item: item.clone(), at: ev.position });
                    cx.stop_propagation();
                    cx.notify();
                })
            })
            .on_drag(drag::CardDrag { item: item.clone(), thumb: drag_thumb }, |drag, offset, _, cx| {
                cx.new(|_| drag::DragGhost::new(drag, offset))
            })
            .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                this.select(item.clone(), window, cx);
                if ev.click_count() >= 2 {
                    this.open_lightbox(window, cx);
                }
            }));

        // Animated edges: on hover the edge flows between the purity color
        // and the image's own; at rest, NSFW edges breathe slowly.
        let edge = purity.as_deref().map(purity_color);
        match (selected, edge, hovered, purity.as_deref()) {
            (false, Some(c), true, _) => {
                let own = glass::glow_color(&colors).unwrap_or(c);
                card.with_animation(
                    SharedString::from(format!("edge-flow-{card_key}")),
                    Animation::new(std::time::Duration::from_millis(1600))
                        .repeat()
                        .with_easing(pulsating_between(0., 1.))
                        .with_max_fps(30.),
                    move |d, t| d.border_color(mix(c, own, t)),
                )
                .into_any_element()
            }
            (false, Some(c), false, Some("nsfw")) => card
                .with_animation(
                    SharedString::from(format!("edge-breathe-{card_key}")),
                    Animation::new(std::time::Duration::from_millis(2400))
                        .repeat()
                        .with_easing(pulsating_between(0.35, 0.95))
                        .with_max_fps(20.),
                    move |d, t| d.border_color(with_alpha(c, t)),
                )
                .into_any_element(),
            _ => card.into_any_element(),
        }
    }

    /// A row of shimmering placeholder cards.
    fn skeleton_row(&self, row: usize) -> Div {
        let GridLayout { cols, card_w, card_h } = self.grid_layout;
        let p = pal();
        h_flex().px(px(GRID_PAD)).gap(px(GRID_GAP)).children((0..cols).map(|i| {
            div()
                .relative()
                .w(px(card_w))
                .h(px(card_h))
                .flex_none()
                .rounded_xl()
                .overflow_hidden()
                .bg(p.surface2)
                .child(shimmer(SharedString::from(format!("skeleton-{row}-{i}"))))
        }))
    }

    fn render_footer(&mut self, row_h: f32, cx: &mut Context<Self>) -> AnyElement {
        let p = pal();
        let more = self.tab == Tab::Discover && self.page < self.last_page;
        if more && self.status_error && !self.searching {
            return h_flex()
                .w_full()
                .h(px(row_h))
                .justify_center()
                .items_start()
                .pt_4()
                .gap_3()
                .text_xs()
                .text_color(p.muted)
                .child(icon("circle-alert").size_4().text_color(p.error))
                .child("Couldn't load more")
                .child(
                    small_button("more", "Retry", BtnKind::Secondary)
                        .on_click(cx.listener(|this, _, _, cx| this.search(false, cx))),
                )
                .into_any_element();
        }
        if more || self.searching {
            // Next page streams in behind these.
            return div().h(px(row_h)).child(self.skeleton_row(usize::MAX)).into_any_element();
        }

        let count = self.current_items().len();
        let sub = match self.tab {
            Tab::Discover => format!("{count} wallpapers for these filters"),
            Tab::Library | Tab::History => format!("{count} wallpapers in your library"),
            Tab::Galleries => format!("{count} wallpapers in this gallery"),
        };
        let line = |flip: bool| {
            let (a, b) = if flip {
                (p.border_strong, with_alpha(p.border_strong, 0.))
            } else {
                (with_alpha(p.border_strong, 0.), p.border_strong)
            };
            div().flex_1().h(px(1.)).bg(linear_gradient(90., linear_color_stop(a, 0.), linear_color_stop(b, 1.)))
        };
        v_flex()
            .w_full()
            .h(px(row_h))
            .items_center()
            .pt_6()
            .gap_3()
            .child(
                h_flex()
                    .w(px(360.))
                    .gap_3()
                    .child(line(false))
                    .child(
                        div()
                            .size(px(30.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(with_alpha(p.accent, 0.14))
                            .border_1()
                            .border_color(with_alpha(p.accent, 0.4))
                            .child(icon("check").size_4().text_color(p.accent)),
                    )
                    .child(line(true)),
            )
            .child(
                v_flex()
                    .items_center()
                    .gap_0p5()
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("You've seen it all"))
                    .child(div().text_xs().text_color(p.dim).child(sub)),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(small_button("back-top", "Back to top", BtnKind::Secondary).on_click(cx.listener(
                        |this, _, _, cx| {
                            this.grid_scroll.scroll_to_item(0, ScrollStrategy::Top);
                            cx.notify();
                        },
                    )))
                    .when(self.tab == Tab::Discover && self.sorting != Sorting::Random, |d| {
                        d.child(small_button("shuffle", "Shuffle", BtnKind::Secondary).on_click(cx.listener(
                            |this, _, _, cx| {
                                this.sorting = Sorting::Random;
                                this.search(true, cx);
                            },
                        )))
                    }),
            )
            .into_any_element()
    }

    fn render_empty(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = pal();
        if self.tab == Tab::Discover && self.searching {
            return v_flex()
                .flex_1()
                .min_h_0()
                .pt(px(GRID_PAD))
                .gap(px(GRID_GAP))
                .overflow_hidden()
                .children((0..4).map(|r| self.skeleton_row(r)))
                .into_any_element();
        }

        let hero = |name: &str, c: Hsla| {
            div()
                .size(px(64.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(with_alpha(c, 0.12))
                .border_1()
                .border_color(with_alpha(c, 0.35))
                .child(icon(name).size(px(26.)).text_color(c))
        };
        let fix = |id: ElementId, label: String| {
            h_flex()
                .id(id)
                .gap_1p5()
                .px_3()
                .h(px(28.))
                .rounded_full()
                .bg(p.surface2)
                .border_1()
                .border_color(p.border)
                .text_xs()
                .text_color(p.muted)
                .cursor_pointer()
                .hover(move |s| s.border_color(p.accent).text_color(p.fg))
                .child(label)
                .child(icon("close").size_3())
        };
        let shell = |hero_el: Div, title: &str, sub: String| {
            v_flex().flex_1().min_h_0().items_center().pt(px(140.)).px_8().gap_4().child(hero_el).child(
                v_flex()
                    .items_center()
                    .gap_1()
                    .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child(title.to_string()))
                    .child(div().max_w(px(420.)).text_sm().text_color(p.dim).child(sub)),
            )
        };

        match self.tab {
            Tab::Galleries => self.render_gallery_empty(cx),
            Tab::Library | Tab::History => shell(
                hero("folder-open", p.accent),
                "Your library is empty",
                "Wallpapers you download or set show up here.".into(),
            )
            .child(button("browse", "Browse Wallhaven", BtnKind::Primary, p).on_click(cx.listener(|this, _, _, cx| {
                this.tab = Tab::Discover;
                cx.notify();
            })))
            .into_any_element(),
            Tab::Discover if self.status_error => {
                shell(hero("triangle-alert", p.error), "Couldn't reach Wallhaven", self.status.to_string())
                    .child(
                        button("retry-search", "Try again", BtnKind::Primary, p)
                            .on_click(cx.listener(|this, _, _, cx| this.search(true, cx))),
                    )
                    .into_any_element()
            }
            Tab::Discover => {
                let query = self.query.read(cx).value().to_string();
                let parsed = smartq::parse(&query);
                let mut fixes: Vec<AnyElement> = Vec::new();
                if let Some(scope) = self.scope.clone() {
                    let (_, name, _) = self.scope_label(&scope, cx);
                    fixes.push(
                        fix("fix-scope".into(), name)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.scope = None;
                                this.search(true, cx);
                            }))
                            .into_any_element(),
                    );
                }
                // Smart-query filters first: they're usually what narrowed it.
                let filter_words: Vec<String> = parsed
                    .chips
                    .iter()
                    .filter(|c| c.kind != smartq::ChipKind::Operator)
                    .flat_map(|c| c.words.clone())
                    .collect();
                for (i, chip) in parsed.chips.iter().enumerate() {
                    if chip.kind == smartq::ChipKind::Operator {
                        continue;
                    }
                    let words = chip.words.clone();
                    let raw = query.clone();
                    fixes.push(
                        fix(("fix-smart", i).into(), chip.label.clone())
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let rest = smartq::without(&raw, &words);
                                this.query.update(cx, |s, cx| s.set_value(rest, window, cx));
                                this.search(true, cx);
                            }))
                            .into_any_element(),
                    );
                }
                if !parsed.text.is_empty() {
                    let keep = filter_words.join(" ");
                    fixes.push(
                        fix("fix-query".into(), format!("Search \"{}\"", parsed.text))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                let keep = keep.clone();
                                this.query.update(cx, |s, cx| s.set_value(keep, window, cx));
                                this.search(true, cx);
                            }))
                            .into_any_element(),
                    );
                }
                if let Some(c) = &self.color {
                    fixes.push(
                        fix("fix-color".into(), format!("Color #{c}"))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.color = None;
                                this.search(true, cx);
                            }))
                            .into_any_element(),
                    );
                }
                if let Some(r) = &self.atleast {
                    fixes.push(
                        fix("fix-res".into(), format!("At least {r}"))
                            .on_click(cx.listener(|this, _, _, cx| this.set_min_resolution(None, cx)))
                            .into_any_element(),
                    );
                }
                if self.resolutions.is_some() {
                    fixes.push(
                        fix("fix-exact".into(), "Exact resolutions".into())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.resolutions = None;
                                this.search(true, cx);
                            }))
                            .into_any_element(),
                    );
                }
                if let Some(r) = &self.ratios {
                    fixes.push(
                        fix("fix-ratio".into(), format!("Ratio {}", r.replace(',', ", ")))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.ratios = None;
                                this.search(true, cx);
                            }))
                            .into_any_element(),
                    );
                }
                if !self.categories.iter().all(|b| *b) {
                    let only: Vec<&str> =
                        CATEGORIES.iter().zip(self.categories).filter_map(|(c, on)| on.then_some(*c)).collect();
                    fixes.push(
                        fix("fix-cats".into(), format!("Only {}", only.join(" + ")))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.categories = [true; 3];
                                this.search(true, cx);
                            }))
                            .into_any_element(),
                    );
                }
                let has_fixes = !fixes.is_empty();
                shell(
                    hero("search", p.accent),
                    "Nothing matches",
                    if has_fixes {
                        "Wallhaven has nothing for this combination. Drop a filter:".into()
                    } else {
                        "Wallhaven has nothing for this combination.".into()
                    },
                )
                .when(has_fixes, |d| {
                    d.child(h_flex().flex_wrap().justify_center().max_w(px(520.)).gap_2().children(fixes))
                })
                .child(
                    button("reset-all", "Reset all filters", BtnKind::Secondary, p)
                        .on_click(cx.listener(|this, _, _, cx| this.reset_filters(cx))),
                )
                .into_any_element()
            }
        }
    }

    /// One virtualized row of cards (or the footer, as the last row).
    fn render_grid_row(&mut self, row: usize, cx: &mut Context<Self>) -> AnyElement {
        let GridLayout { cols, card_w, card_h } = self.grid_layout;
        let items = self.current_items();
        let mosaic = self.cfg.layout == GridMode::Mosaic;

        let (start, row_h, cards): (usize, f32, Vec<AnyElement>) = if mosaic {
            let row_h = self.justified_h + GRID_GAP;
            let Some(layout) = self.justified.get(row).cloned() else {
                return self.render_footer(row_h, cx);
            };
            let start = layout.first().map(|(ix, _)| *ix).unwrap_or(0);
            let h = self.justified_h;
            let cards = layout
                .into_iter()
                .filter_map(|(ix, w)| Some(self.render_card(ix, items.get(ix)?.clone(), w, h, cx)))
                .collect();
            (start, row_h, cards)
        } else {
            let start = row * cols;
            let row_h = card_h + GRID_GAP;
            if start >= items.len() {
                return self.render_footer(row_h, cx);
            }
            let cards = items
                .into_iter()
                .enumerate()
                .skip(start)
                .take(cols)
                .map(|(ix, item)| self.render_card(ix, item, card_w, card_h, cx))
                .collect();
            (start, row_h, cards)
        };
        let bounds = self.card_bounds.clone();
        h_flex()
            .h(px(row_h))
            .px(px(GRID_PAD))
            .items_start()
            .gap(px(GRID_GAP))
            .children(cards)
            .on_children_prepainted(move |rects, _, _| {
                let mut map = bounds.borrow_mut();
                let mine = start..start + rects.len();
                // Evict rects of cards that used to be painted here.
                map.retain(|ix, b| mine.contains(ix) || !rects.iter().any(|r| r.intersects(b)));
                for (k, rect) in rects.into_iter().enumerate() {
                    map.insert(start + k, rect);
                }
            })
            .into_any_element()
    }

    fn render_grid(&mut self, grid_w: f32, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let count = self.current_items().len();

        let cols = (((grid_w + GRID_GAP) / (MIN_CARD_W + GRID_GAP)).floor() as usize).max(1);
        let card_w = ((grid_w - GRID_GAP * (cols - 1) as f32) / cols as f32).floor();
        let card_h = (card_w * 0.625).floor();
        self.grid_layout = GridLayout { cols, card_w, card_h };

        if count == 0 {
            return self.render_empty(cx);
        }

        let mosaic = self.cfg.layout == GridMode::Mosaic;
        if mosaic {
            let items = self.current_items();
            let aspects: Vec<f32> = items.iter().map(|i| self.item_aspect(i)).collect();
            self.justified_h = (grid_w / 4.6).clamp(160., 240.).floor();
            self.justified = gridmodes::justify(&aspects, grid_w, self.justified_h, GRID_GAP);
        }

        // +1 row for the footer (spinner / load more / end marker).
        let rows = if mosaic { self.justified.len() + 1 } else { count.div_ceil(cols) + 1 };
        // Rows repaint each frame; stale rects would mislead the predictor.
        self.card_bounds.borrow_mut().clear();
        div()
            .id("grid-area")
            .flex_1()
            .min_h_0()
            .pt(px(GRID_PAD))
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| this.precog_move(ev.position, cx)))
            .on_hover(cx.listener(|this, hovered: &bool, _, _| {
                if !hovered {
                    this.precog.last = None;
                    this.precog.hover = None;
                }
            }))
            .child(
                uniform_list(
                    "grid",
                    rows,
                    cx.processor(move |this, range: std::ops::Range<usize>, window, cx| {
                        // Infinite scroll: fetch the next page as the footer nears.
                        if this.tab == Tab::Discover
                            && !this.searching
                            && this.page < this.last_page
                            && range.end + 3 >= rows
                        {
                            cx.on_next_frame(window, |this, _, cx| this.search(false, cx));
                        }
                        range.map(|row| this.render_grid_row(row, cx)).collect::<Vec<_>>()
                    }),
                )
                .size_full()
                .track_scroll(&self.grid_scroll),
            )
            .into_any_element()
    }

    // ---- inspector ----------------------------------------------------------

    /// Large thumbnail underneath, full-resolution original on top once it
    /// has downloaded and decoded.
    fn render_preview(&mut self, item: &Item, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = pal();
        let remote = match item {
            Item::Remote(w) => Some(w.clone()),
            Item::Local(_) => None,
        };
        let (base, full, key) = match item {
            Item::Remote(w) => (self.remote_thumb(w, true), self.remote_full(w, cx), w.id.clone()),
            Item::Local(path) => (self.local_thumb(path), Some(path.clone()), local_key(path)),
        };
        let phase = self.full_phase(item);
        // Show a downscale made for exactly this box; the raw original would
        // alias when the GPU shrinks it 10-20x.
        let fit_w = ((INSPECTOR_W - INSPECTOR_PAD * 2.) * self.scale).round() as u32;
        let preview_h = self.preview_h();
        let fit_h = (preview_h * self.scale).round() as u32;
        let fit = full.as_deref().and_then(|f| self.fitted(f, fit_w, fit_h));
        // Gray only while the original is still on the wire.
        let bands = match &phase {
            FullPhase::Loading(b) => Some(*b),
            _ => None,
        };
        let developing = bands.is_some();
        let no_data = bands.is_none_or(|b| b.mask == 0 && b.fraction.unwrap_or(0.) < 1. / 16.);
        let base_missing = base.is_none() && fit.is_none();
        let xray = self.xray_overlay();
        let band_layers = match (bands, &base) {
            (Some(b), Some(t)) => develop_bands(&key, t, b, 12.),
            _ => Vec::new(),
        };

        div()
            .image_cache(self.preview_cache.clone())
            .id("preview")
            .group("preview")
            .relative()
            .w_full()
            .h(px(preview_h))
            .rounded_xl()
            .overflow_hidden()
            .bg(hex(0x000000))
            .cursor_pointer()
            .on_click(cx.listener(|this, _, window, cx| this.open_lightbox(window, cx)))
            // The thumbnail "develops": monochrome and dimmed until the original arrives.
            .when_some(base, |d, t| {
                d.child(radius(img(t).size_full().object_fit(ObjectFit::Contain).grayscale(developing), 12.))
            })
            .when(developing || base_missing, |d| {
                d.child(radius(div().absolute().top_0().left_0().size_full().bg(hsla(0., 0., 0., 0.35)), 12.))
            })
            .children(band_layers)
            .when((developing && no_data) || base_missing, |d| {
                d.child(shimmer(SharedString::from(format!("shimmer-{key}"))))
            })
            .when_some(fit, |d, f| {
                d.child(fade_in(
                    SharedString::from(format!("fade-{key}")),
                    radius(img(f).size_full().object_fit(ObjectFit::Contain), 12.),
                ))
            })
            // Color X-ray: everything but the hovered palette color, darkened.
            .when_some(xray, |d, mask| {
                d.child(radius(img(mask).absolute().top_0().left_0().size_full().object_fit(ObjectFit::Contain), 12.))
            })
            .map(|d| match phase.clone() {
                FullPhase::Done => d,
                FullPhase::Loading(Bands { fraction, .. }) => d.child(
                    v_flex()
                        .absolute()
                        .left_3()
                        .right_3()
                        .bottom_3()
                        .gap_1p5()
                        .items_end()
                        .children(fraction.map(|f| {
                            div()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(hsla(0., 0., 1., 0.8))
                                .child(format!("{}%", (f * 100.) as u32))
                        }))
                        .child(progress_line(SharedString::from(format!("sweep-{key}")), fraction, 3., p)),
                ),
                FullPhase::Failed(e) => d.child(
                    h_flex()
                        .id("full-retry")
                        .absolute()
                        .left_3()
                        .right_3()
                        .bottom_3()
                        .gap_2()
                        .px_3()
                        .py_1p5()
                        .rounded_lg()
                        .bg(hsla(0., 0., 0., 0.7))
                        .text_xs()
                        .child(icon("circle-alert").size_3p5().text_color(p.error))
                        .child(div().flex_1().min_w_0().truncate().text_color(hsla(0., 0., 1., 0.8)).child(e))
                        .child(div().text_color(p.accent).font_weight(FontWeight::MEDIUM).child("Retry"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            cx.stop_propagation();
                            this.retry_full(cx);
                        })),
                ),
            })
            // Actions float on the preview while hovered, out of the way otherwise.
            .child(
                h_flex()
                    .absolute()
                    .top_2()
                    .right_2()
                    .gap_0p5()
                    .p_0p5()
                    .rounded_lg()
                    .bg(hsla(0., 0., 0., 0.6))
                    .invisible()
                    .group_hover("preview", |s| s.visible())
                    .children(remote.clone().map(|w| {
                        let like = format!("like:{}", w.id);
                        kit::Btn::new("pv-like")
                            .icon("search")
                            .ghost()
                            .sm()
                            .tooltip("More like this")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.search_for(like.clone(), window, cx)
                            }))
                            .render()
                    }))
                    .child(
                        kit::Btn::new("pv-full")
                            .icon("maximize")
                            .ghost()
                            .sm()
                            .tooltip("Full size")
                            .on_click(cx.listener(|this, _, window, cx| {
                                cx.stop_propagation();
                                this.open_lightbox(window, cx)
                            }))
                            .render(),
                    )
                    .children(remote.clone().map(|w| {
                        let url = w.url.clone();
                        kit::Btn::new("pv-open")
                            .icon("external-link")
                            .ghost()
                            .sm()
                            .tooltip("Open on wallhaven.cc")
                            .on_click(move |_, _, cx| {
                                cx.stop_propagation();
                                cx.open_url(&url)
                            })
                            .render()
                    }))
                    .children(remote.map(|_| {
                        kit::Btn::new("pv-save")
                            .icon("arrow-down")
                            .ghost()
                            .sm()
                            .tooltip("Save to library")
                            .on_click(cx.listener(|this, _, _, cx| {
                                cx.stop_propagation();
                                this.download_selected(cx)
                            }))
                            .render()
                    })),
            )
    }

    /// Preview + title (above the Displays card) and the remaining details
    /// (below it), so long tag lists never push the display controls down.
    fn render_details(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = pal();
        let Some(item) = self.selected.clone() else {
            let empty = v_flex()
                .items_center()
                .justify_center()
                .gap_2()
                .h(px(PREVIEW_H))
                .rounded_xl()
                .border_1()
                .border_color(p.border)
                .bg(p.bg)
                .child(icon("frame").size(px(26.)).text_color(p.dim))
                .child(div().text_color(p.muted).child("Select a wallpaper"))
                .child(div().text_xs().text_color(p.dim).child("Its full-resolution preview shows here"))
                .into_any_element();
            return empty;
        };
        let preview = self.render_preview(&item, cx);
        let save = self.gallery_button("inspector-gallery", &item, cx).sm();

        match item {
            Item::Local(path) => v_flex()
                .gap_3()
                .child(preview)
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .font_weight(FontWeight::SEMIBOLD)
                                .truncate()
                                .child(file_name(&path)),
                        )
                        .child(save),
                )
                .into_any_element(),
            Item::Remote(w) => {
                let pc = purity_color(&w.purity);
                let head = v_flex()
                    .gap_3()
                    .child(preview)
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().text_base().font_weight(FontWeight::SEMIBOLD).child(w.id.clone()))
                            .child(
                                h_flex()
                                    .gap_1()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_full()
                                    .bg(with_alpha(pc, 0.15))
                                    .text_xs()
                                    .text_color(pc)
                                    .child(div().size(px(6.)).rounded_full().bg(pc))
                                    .child(purity_label(&w.purity)),
                            )
                            .child(div().text_xs().text_color(p.dim).child(w.category.clone()))
                            .child(div().flex_1())
                            .child(
                                h_flex()
                                    .gap_1()
                                    .text_xs()
                                    .text_color(p.muted)
                                    .child(icon("heart").size_3p5())
                                    .child(compact_count(w.favorites)),
                            )
                            .child(save),
                    )
                    .into_any_element();
                head
            }
        }
    }

    fn render_inspector(&mut self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = pal();
        let details = self.render_details(cx);
        let title = match &self.selected {
            Some(Item::Remote(w)) => format!("wallhaven-{}", w.id),
            Some(Item::Local(path)) => file_name(path),
            None => "Details".into(),
        };
        let enter_key = match &self.selected {
            Some(Item::Remote(w)) => w.id.clone(),
            Some(Item::Local(path)) => local_key(path),
            None => "none".into(),
        };

        // One thing at a time below the preview: Set, Info, or Similar.
        let selected = self.selected.clone();
        let tab = if selected.is_none() { InspectorTab::Set } else { self.inspector_tab };
        let similar_count = match &selected {
            Some(Item::Remote(w)) => match self.similar.get(&w.id) {
                Some(details::Similar::Ready(list)) => Some(list.len()),
                _ => None,
            },
            _ => None,
        };
        let tab_seg = |id: &'static str, label: String, t: InspectorTab| {
            segment(id, label, tab == t, p).on_click(cx.listener(move |this, _, _, cx| {
                this.inspector_tab = t;
                cx.notify();
            }))
        };
        let tabs = selected.as_ref().map(|item| {
            let remote = matches!(item, Item::Remote(_));
            segmented_track(p)
                .child(tab_seg("it-set", "Use".into(), InspectorTab::Set))
                .child(tab_seg("it-info", "Info".into(), InspectorTab::Info))
                .when(remote, |d| {
                    d.child(tab_seg(
                        "it-similar",
                        match similar_count {
                            Some(n) if n > 0 => format!("Similar  {n}"),
                            _ => "Similar".into(),
                        },
                        InspectorTab::Similar,
                    ))
                })
        });
        let content: AnyElement = match (tab, &selected) {
            (InspectorTab::Info, Some(Item::Remote(w))) => {
                let w = w.clone();
                self.render_info_tab(&w, cx)
            }
            (InspectorTab::Info, Some(Item::Local(path))) => {
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
            (InspectorTab::Similar, Some(Item::Remote(w))) => {
                let w = w.clone();
                self.render_similar(&w, cx).unwrap_or_else(|| {
                    div().text_sm().text_color(p.dim).child("No similar wallpapers found").into_any_element()
                })
            }
            _ => self.render_put_on(cx),
        };
        let tab_key = match tab {
            InspectorTab::Set => "set",
            InspectorTab::Info => "info",
            InspectorTab::Similar => "similar",
        };
        let details = glass::stagger(SharedString::from(format!("enter-{enter_key}-0")), 0, details);
        let content = glass::stagger(SharedString::from(format!("enter-{enter_key}-{tab_key}")), 1, content);

        let backdrop = self.backdrop();
        v_flex()
            .relative()
            .overflow_hidden()
            .w(px(INSPECTOR_W))
            .h_full()
            .flex_none()
            .bg(p.surface)
            .border_l_1()
            .border_color(p.border)
            .children(glass::glass_layers(backdrop, "inspector", p))
            .child(
                title_region(h_flex(), p)
                    .pl(px(INSPECTOR_PAD))
                    .pr_3()
                    .gap_2()
                    .child(div().flex_1().min_w_0().truncate().text_xs().text_color(p.dim).child(title))
                    .when(!NATIVE_WINDOW_BUTTONS, |d| d.child(window_controls(window, p))),
            )
            .child(
                div()
                    .id("inspector")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(v_flex().gap_4().p(px(INSPECTOR_PAD)).child(details).children(tabs).child(content)),
            )
    }

    // ---- settings -----------------------------------------------------------

    // ---- precognition ---------------------------------------------------------

    /// Card whose painted bounds contain `point`.
    fn card_at(&self, point: Point<Pixels>) -> Option<usize> {
        self.card_bounds.borrow().iter().find(|(_, b)| b.contains(&point)).map(|(ix, _)| *ix)
    }

    /// Read the pointer's trajectory over the grid and start fetching the
    /// original of the card it's heading for, before it gets there.
    fn precog_move(&mut self, pos: Point<Pixels>, cx: &mut Context<Self>) {
        if !self.cfg.precog {
            return;
        }
        let now = Instant::now();
        let pg = &mut self.precog;
        if let Some((last, at)) = pg.last {
            let dt = (now - at).as_secs_f32();
            if dt > 0.0005 && dt < 0.2 {
                let vx = f32::from(pos.x - last.x) / dt;
                let vy = f32::from(pos.y - last.y) / dt;
                // Smooth out jitter.
                pg.vel = (pg.vel.0 * 0.6 + vx * 0.4, pg.vel.1 * 0.6 + vy * 0.4);
            } else {
                pg.vel = (0., 0.);
            }
        }
        pg.last = Some((pos, now));
        let speed = (pg.vel.0 * pg.vel.0 + pg.vel.1 * pg.vel.1).sqrt();

        // Where the pointer will be in ~200ms if it keeps going.
        let ahead = point(pos.x + px(pg.vel.0 * PRECOG_LOOKAHEAD), pos.y + px(pg.vel.1 * PRECOG_LOOKAHEAD));
        let target = if speed > 40. { self.card_at(ahead) } else { None };
        let under = self.card_at(pos);

        let pg = &mut self.precog;
        if target.is_some() && target == pg.predicted {
            pg.streak = pg.streak.saturating_add(1);
            pg.peak = pg.peak.max(speed);
        } else {
            pg.predicted = target;
            pg.streak = 1;
            pg.peak = speed;
        }
        // A committed move: a fast approach that is now braking onto a card
        // the prediction has held on for several samples.
        let braking = pg.peak > 300. && speed < pg.peak * 0.45;
        if let Some(ix) = target.filter(|_| pg.streak >= 3 && braking) {
            self.prefetch(ix, cx);
        }

        // Dwell: resting on a card for a moment counts too.
        if under != self.precog.hover {
            self.precog.hover = under;
            if let Some(ix) = under {
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(std::time::Duration::from_millis(PRECOG_DWELL_MS)).await;
                    this.update(cx, |this, cx| {
                        if this.precog.hover == Some(ix) {
                            this.prefetch(ix, cx);
                        }
                    })
                    .ok();
                })
                .detach();
            }
        }
    }

    /// Speculatively fetch the original for card `ix`.
    fn prefetch(&mut self, ix: usize, cx: &mut Context<Self>) {
        if !self.cfg.precog {
            return;
        }
        let Some(Item::Remote(w)) = self.current_items().into_iter().nth(ix) else {
            return;
        };
        if self.full.contains_key(&w.id) || self.cfg.download_dir.join(w.file_name()).is_file() {
            return;
        }
        if remote_cache_path(&full_key(&w), &w.path).is_file() {
            return;
        }
        // Keep a few guesses in flight at most; drop the stalest.
        let selected_id = match &self.selected {
            Some(Item::Remote(s)) => Some(s.id.clone()),
            _ => None,
        };
        let full = &self.full;
        self.precog.speculative.retain(|id| matches!(full.get(id), Some(FullState::Loading { .. })));
        while self.precog.speculative.len() >= PRECOG_MAX_INFLIGHT {
            let Some(old) = self.precog.speculative.pop_front() else { break };
            if Some(&old) != selected_id.as_ref() {
                if let Some(FullState::Loading { progress }) = self.full.remove(&old) {
                    progress.cancel.store(true, Ordering::Relaxed);
                }
            }
        }
        self.precog.speculative.push_back(w.id.clone());
        self.precog.prefetched.insert(w.id.clone());
        self.precog.started += 1;
        self.start_full(w, PRECOG_WORKERS, cx);
    }
}

impl Render for Muralist {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let target =
            if self.cfg.adaptive_theme { adaptive::derive(&self.selected_colors()) } else { adaptive::base_pal() };
        let (now_pal, animating) = self.theme_anim.step(target);
        if now_pal != adaptive::current() {
            adaptive::set_current(now_pal);
            apply_theme(cx);
        }
        if animating {
            window.request_animation_frame();
        }
        let p = pal();
        let viewport_w = f32::from(window.viewport_size().width);
        self.scale = window.scale_factor();
        self.search_focused = self.query.read(cx).focus_handle(cx).is_focused(window);
        self.viewport = (viewport_w, f32::from(window.viewport_size().height));
        let frame = if matches!(window.window_decorations(), Decorations::Client { .. }) && !window.is_maximized() {
            // gpui-component's window border: shadow on both sides plus a 1px edge.
            2. * 21.
        } else {
            0.
        };
        self.content = (viewport_w - frame, f32::from(window.viewport_size().height) - frame);
        let grid_w = (viewport_w - frame - SIDEBAR_W - INSPECTOR_W - GRID_PAD * 2. - SCROLLBAR_W).max(MIN_CARD_W);

        let sidebar = self.render_sidebar(cx);
        let topbar = self.render_topbar(cx);
        let query_chips = if self.tab == Tab::Discover { self.render_query_chips(cx) } else { None };
        let recent = if self.tab == Tab::Discover { self.render_recent(cx) } else { None };
        let stage = self.render_stage(viewport_w - frame - SIDEBAR_W - INSPECTOR_W, cx);
        let grid = if self.tab == Tab::History {
            self.render_history(cx)
        } else {
            self.render_grid(grid_w, cx).into_any_element()
        };
        let toast = self.render_toast(cx);
        let inspector = self.render_inspector(window, cx);
        // Focus lost mid-peek means Space-up will never arrive.
        if self.peek.is_some() && !window.is_window_active() {
            self.peek_end(cx);
        }
        let peek_badge = self.render_peek_badge();
        let lightbox = self.lightbox.then(|| self.render_lightbox(window, cx));
        let settings = self.settings_open.then(|| self.render_settings(cx));
        let picker = self.render_picker(cx);
        let span_editor = self.render_span_editor(cx);

        h_flex()
            .id("root")
            .track_focus(&self.main_focus)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                if ev.keystroke.key == "space" && !this.typing(window, cx) {
                    if !ev.is_held {
                        this.peek_start(cx);
                    }
                    cx.stop_propagation();
                }
            }))
            .on_key_up(cx.listener(|this, ev: &KeyUpEvent, _, cx| {
                if ev.keystroke.key == "space" && this.peek.is_some() {
                    this.peek_end(cx);
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    // Clicking empty UI takes focus back from fields, so Space peeks.
                    if !this.typing(window, cx) && !this.lightbox && !this.settings_open {
                        this.main_focus.focus(window, cx);
                    }
                }),
            )
            .relative()
            .size_full()
            .items_start()
            .bg(p.bg)
            .text_color(p.fg)
            .text_sm()
            .child(sidebar)
            .child(
                v_flex()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(topbar)
                    .children(query_chips)
                    .child(grid)
                    .child(stage)
                    .children(recent),
            )
            .child(inspector)
            .children(lightbox)
            .children(picker)
            .children(toast)
            .children(peek_badge)
            .children(span_editor)
            .children(settings)
    }
}

#[cfg(test)]
mod tests {
    use super::thousands;

    #[test]
    fn thousands_groups_digits() {
        assert_eq!(thousands(7), "7");
        assert_eq!(thousands(13_059), "13,059");
        assert_eq!(thousands(626_729), "626,729");
        assert_eq!(thousands(1_000_000), "1,000,000");
    }
}
