//! The composer: the one place a wallpaper gets set. It shows your real
//! desk: put the picture on every screen (each framed on its own: click one,
//! then drag to move it, scroll to zoom, turn or mirror it), on one screen
//! only (the others keep what they show), or spanned across them all; pick
//! desktop, lock screen or both; Set puts it up there and then. The
//! monitors sit at true proportions with bezel gaps; around the screen
//! being framed the rest of the picture shows dimmed, so you can see what
//! gets cut.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    BtnKind, Item, Muralist, Target, button, icon, icon_button, kit, pal, segment, segmented_track, with_alpha,
};
use crate::config::cache_dir;
use crate::span::{self, Framing, Orient};
use crate::thumbs::Job;

/// Room around the desktop where overhanging image shows, dimmed.
const MARGIN: f32 = 44.;
const MAX_ZOOM: f32 = 4.;
const GAP: f32 = 6.;

/// Where the picture goes: every screen (each framed on its own), one
/// screen, or one picture across them all.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ComposeMode {
    Every,
    One,
    Span,
}

pub(super) struct SpanEdit {
    item: Item,
    dims: (u32, u32),
    mode: ComposeMode,
    span: Framing,
    screens: BTreeMap<String, Framing>,
    /// The screen being framed (Every) or set (One).
    active: String,
    exit: ComposeExit,
    drag: Option<Point<Pixels>>,
}

/// What finishing the composer does: from the side panel it arranges the
/// plan in step 3 (Set in step 4 puts it up); from the lightbox or the
/// right-click card, where those steps aren't in view, it sets straight away.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ComposeExit {
    Plan,
    Set,
}

/// How a wallpaper was last composed, so the composer reopens on it as you
/// left it instead of putting the same picture back on every screen.
#[derive(Clone)]
pub(super) struct ComposeMemory {
    mode: ComposeMode,
    span: Framing,
    screens: BTreeMap<String, Framing>,
    active: String,
}

/// What the composer remembers a wallpaper by.
fn memory_key(item: &Item) -> String {
    match item {
        Item::Remote(w) => w.id.clone(),
        Item::Local(path) => path.to_string_lossy().into_owned(),
    }
}

impl SpanEdit {
    /// The framing the tools act on: the active screen's, or the span's.
    fn framing_mut(&mut self) -> &mut Framing {
        match self.mode {
            ComposeMode::Span => &mut self.span,
            ComposeMode::Every | ComposeMode::One => self.screens.entry(self.active.clone()).or_default(),
        }
    }

    fn framing(&self) -> Framing {
        match self.mode {
            ComposeMode::Span => self.span,
            ComposeMode::Every | ComposeMode::One => self.screens.get(&self.active).copied().unwrap_or_default(),
        }
    }
}

/// `framing` with its pan moved by a drag of (`dx`, `dy`) desktop units, for
/// a picture `dims` covering a box `bw` x `bh`.
fn dragged(framing: Framing, dims: (u32, u32), bw: f64, bh: f64, dx: f64, dy: f64) -> Framing {
    let (s, off_x, off_y) = span::layout(dims.0, dims.1, bw, bh, framing);
    let slack_x = (f64::from(dims.0) * s - bw).max(0.);
    let slack_y = (f64::from(dims.1) * s - bh).max(0.);
    // Dragging right reveals more of the left: the overhang shrinks.
    let new_x = (off_x - dx).clamp(0., slack_x);
    let new_y = (off_y - dy).clamp(0., slack_y);
    let to_pan = |off: f64, slack: f64| if slack > 0. { (off / (slack / 2.) - 1.) as f32 } else { 0. };
    Framing { pan: (to_pan(new_x, slack_x), to_pan(new_y, slack_y)), ..framing }
}

impl Muralist {
    /// Opens the composer on one picture across every screen.
    pub(super) fn open_span_editor(&mut self, exit: ComposeExit, cx: &mut Context<Self>) {
        self.open_composer(ComposeMode::Span, None, exit, cx);
    }

    /// Opens the composer for the selected wallpaper; `screen` picks which
    /// one is framed (or set, in One mode), the primary otherwise.
    pub(super) fn open_composer(
        &mut self,
        mode: ComposeMode,
        screen: Option<String>,
        exit: ComposeExit,
        cx: &mut Context<Self>,
    ) {
        let (Some(item), Some(dims)) = (self.selected.clone(), self.selected_dims) else {
            // Unknown size: nothing to frame, so put it up as it is.
            if let Some(item) = self.selected.clone() {
                let placement = match (mode, screen) {
                    (ComposeMode::One, Some(name)) => super::Placement::Monitor(name),
                    (ComposeMode::Span, _) => super::Placement::Span,
                    _ => super::Placement::All,
                };
                self.assign_item_then(item, self.target.targets(), placement, exit == ComposeExit::Set, cx);
            }
            return;
        };
        let mut order: Vec<_> = self.monitors.iter().collect();
        order.sort_by_key(|m| (!m.primary, m.x));
        let plain_open = screen.is_none();
        let active = screen
            .filter(|n| self.monitors.iter().any(|m| m.name == *n))
            .or_else(|| order.first().map(|m| m.name.clone()))
            .unwrap_or_default();
        // Opened plainly (Set), it comes back exactly as last composed;
        // opened on a particular screen or mode, that's what it shows, with
        // the framings remembered.
        let remembered = self.compose_memory.get(&memory_key(&item)).cloned();
        let (mode, active) = match &remembered {
            Some(m) if mode == ComposeMode::Every && plain_open => (m.mode, m.active.clone()),
            _ => (mode, active),
        };
        let (span, screens) = remembered.map_or_else(|| (Framing::default(), BTreeMap::new()), |m| (m.span, m.screens));
        let mode = if self.monitors.len() < 2 && mode == ComposeMode::Span { ComposeMode::Every } else { mode };
        self.context = None;
        self.span_edit = Some(SpanEdit { item, dims, mode, span, screens, active, exit, drag: None });
        cx.notify();
    }

    /// Remembers how the open composer has this wallpaper.
    fn remember_composition(&mut self) {
        if let Some(edit) = &self.span_edit {
            self.compose_memory.insert(
                memory_key(&edit.item),
                ComposeMemory {
                    mode: edit.mode,
                    span: edit.span,
                    screens: edit.screens.clone(),
                    active: edit.active.clone(),
                },
            );
        }
    }

    fn close_composer(&mut self, cx: &mut Context<Self>) {
        self.remember_composition();
        self.span_edit = None;
        cx.notify();
    }

    fn desktop_box(&self) -> (i32, i32, f64, f64) {
        let (min_x, min_y, max_x, max_y) = crate::monitors::extent(&self.monitors);
        (min_x, min_y, f64::from(max_x - min_x), f64::from(max_y - min_y))
    }

    /// Desktop units to on-screen pixels in the editor.
    fn editor_scale(&self) -> f32 {
        let (_, _, bw, bh) = self.desktop_box();
        let avail_w = (self.viewport.0 - 220.).max(300.);
        let avail_h = (self.viewport.1 - 360.).max(160.);
        (avail_w / bw as f32).min(avail_h / bh as f32)
    }

    fn span_drag(&mut self, pos: Point<Pixels>, cx: &mut Context<Self>) {
        let k = f64::from(self.editor_scale());
        let (_, _, bw, bh) = self.desktop_box();
        let monitors = self.monitors.clone();
        let Some(edit) = &mut self.span_edit else { return };
        let Some(last) = edit.drag else { return };
        let (dx, dy) = (f64::from(f32::from(pos.x - last.x)) / k, f64::from(f32::from(pos.y - last.y)) / k);
        let framing = edit.framing();
        let dims = framing.orient.dims(edit.dims);
        let moved = match edit.mode {
            ComposeMode::Span => dragged(framing, dims, bw, bh, dx, dy),
            ComposeMode::Every | ComposeMode::One => {
                let Some(m) = monitors.iter().find(|m| m.name == edit.active) else { return };
                dragged(framing, dims, f64::from(m.width), f64::from(m.height), dx, dy)
            }
        };
        *edit.framing_mut() = moved;
        edit.drag = Some(pos);
        cx.notify();
    }

    /// Changes the framing being edited.
    fn compose(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Framing)) {
        if let Some(edit) = &mut self.span_edit {
            f(edit.framing_mut());
        }
        cx.notify();
    }

    /// Renders what the composer shows and sets it, there and then.
    fn commit_span(&mut self, cx: &mut Context<Self>) {
        self.remember_composition();
        let Some(edit) = self.span_edit.take() else { return };
        let client = self.client.clone();
        let dir = self.cfg.download_dir.clone();
        let monitors = self.monitors.clone();
        let targets = self.target.targets();
        let SpanEdit { item, mode, span: span_framing, screens, active, exit, .. } = edit;
        // Which screens it goes on, and whether anything needs rendering
        // (an untouched framing is just the picture, filled by the desktop).
        let chosen: Vec<_> = match mode {
            ComposeMode::One => monitors.iter().filter(|m| m.name == active).cloned().collect(),
            _ => monitors.clone(),
        };
        let plain = mode != ComposeMode::Span
            && chosen.iter().all(|m| screens.get(&m.name).is_none_or(|f| *f == Framing::default()));
        self.busy += 1;
        self.set_status(
            match mode {
                ComposeMode::Span => "Slicing across your screens...",
                _ if plain => "Setting...",
                _ => "Framing your screens...",
            },
            false,
            cx,
        );
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let path = Muralist::fetch(&client, &dir, item)?;
                    let out = cache_dir().join("span");
                    match mode {
                        ComposeMode::Span => span::slice_framed(&path, &chosen, &out, span_framing),
                        _ if plain => Ok(chosen.iter().map(|m| (m.name.clone(), path.clone())).collect()),
                        _ => span::frame_each(&path, &chosen, &screens, &out),
                    }
                })
                .await;
            this.update(cx, |this, cx| {
                this.busy -= 1;
                match result {
                    Ok(map) => {
                        for t in &targets {
                            match t {
                                Target::Desktop => this.desktop.extend(map.clone()),
                                Target::Lock => this.lock.extend(map.clone()),
                            }
                        }
                        this.refresh_library();
                        match exit {
                            ComposeExit::Set => this.apply(targets.clone(), cx),
                            ComposeExit::Plan => this.set_status("Arranged. Press Set to put it up.", false, cx),
                        }
                    }
                    Err(e) => this.set_status(format!("Couldn't set it: {e:#}"), true, cx),
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Sharpest copy of the image already on hand for the editor.
    fn editor_image(&mut self, item: &Item, cx: &mut Context<Self>) -> Option<PathBuf> {
        let full = match item {
            Item::Remote(w) => self.remote_full(w, cx),
            Item::Local(path) => Some(path.clone()),
        };
        if let Some(full) = full {
            let bucket = |v: f32| ((v * self.scale / 256.).ceil() * 256.) as u32;
            let (lw, lh) = (bucket(self.viewport.0 - 168.), bucket(self.viewport.1 - 112.));
            if let Some(fit) = self.fitted(&full, lw, lh) {
                return Some(fit);
            }
            let pw = self.fit_width();
            let ph = (self.preview_h() * self.scale).round() as u32;
            if let Some(fit) = self.fitted(&full, pw, ph) {
                return Some(fit);
            }
        }
        match item {
            Item::Remote(w) => self.remote_thumb(w, true),
            Item::Local(path) => self.local_thumb(path),
        }
    }

    /// `base` turned the way `orient` says, rendered once per orientation.
    fn oriented(&mut self, base: &Path, orient: Orient) -> Option<PathBuf> {
        if orient == Orient::default() {
            return Some(base.to_owned());
        }
        let key = format!("orient-{}{}-{}", orient.turns, u8::from(orient.mirror), base.display());
        let path = base.to_owned();
        self.thumb(key, |key| Job::Orient { key, path, orient })
    }

    pub(super) fn render_span_editor(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = pal();
        let (item, dims, mode, span_framing, screens, active) = {
            let edit = self.span_edit.as_ref()?;
            (edit.item.clone(), edit.dims, edit.mode, edit.span, edit.screens.clone(), edit.active.clone())
        };
        let base = self.editor_image(&item, cx);
        let (min_x, min_y, bw, bh) = self.desktop_box();
        let k = self.editor_scale();
        let stage_w = bw as f32 * k + MARGIN * 2.;
        let stage_h = bh as f32 * k + MARGIN * 2.;
        let monitors = self.monitors.clone();
        let accent = p.accent;
        let picture = |path: PathBuf, left: f32, top: f32, w: f32, h: f32| {
            img(path).absolute().left(px(left)).top(px(top)).w(px(w)).h(px(h)).object_fit(ObjectFit::Fill)
        };

        // Where the picture sits (left, top, width, height in stage pixels)
        // for one framing over a box at (bx, by) of bw x bh desktop units.
        let place = |framing: Framing, bx: f32, by: f32, w: f64, h: f64| {
            let (ow, oh) = framing.orient.dims(dims);
            let (s, off_x, off_y) = span::layout(ow, oh, w, h, framing);
            (
                bx - off_x as f32 * k,
                by - off_y as f32 * k,
                (f64::from(ow) * s) as f32 * k,
                (f64::from(oh) * s) as f32 * k,
            )
        };

        let mut backdrop: Option<AnyElement> = None;
        let mut windows: Vec<AnyElement> = Vec::new();
        let span_place = place(span_framing, MARGIN, MARGIN, bw, bh);
        let span_image = base.as_ref().and_then(|b| self.oriented(b, span_framing.orient));
        if mode == ComposeMode::Span
            && let Some(path) = span_image.clone()
        {
            // The whole image, dimmed: everything outside the screens gets cut.
            let (l, t, w, h) = span_place;
            backdrop = Some(picture(path, l, t, w, h).opacity(0.22).into_any_element());
        }
        // In One mode the other screens keep what's on them now.
        let showing: BTreeMap<String, PathBuf> = match self.target.targets().first() {
            Some(Target::Lock) => self.applied.lock.clone(),
            _ => self.applied.desktop.clone(),
        };
        for m in &monitors {
            let mx = MARGIN + (m.x - min_x) as f32 * k;
            let my = MARGIN + (m.y - min_y) as f32 * k;
            let (left, top) = (mx + GAP / 2., my + GAP / 2.);
            let framed_here = mode == ComposeMode::Every || (mode == ComposeMode::One && m.name == active);
            let is_active = mode != ComposeMode::Span && m.name == active;
            // Each screen shows its own framing, its slice of the span, or
            // (in One mode, elsewhere) what's up there already.
            let (framing, (l, t, w, h)) = match mode {
                ComposeMode::Span => (span_framing, span_place),
                _ => {
                    let f = screens.get(&m.name).copied().unwrap_or_default();
                    (f, place(f, mx, my, f64::from(m.width), f64::from(m.height)))
                }
            };
            let current = (!framed_here && mode == ComposeMode::One)
                .then(|| showing.get(&m.name).cloned())
                .flatten()
                .and_then(|path| self.local_thumb(&path));
            let image = match mode {
                ComposeMode::Span => span_image.clone(),
                _ if framed_here => base.as_ref().and_then(|b| self.oriented(b, framing.orient)),
                _ => None,
            };
            if is_active && let Some(path) = image.clone() {
                backdrop = Some(picture(path, l, t, w, h).opacity(0.22).into_any_element());
            }
            let name = m.name.clone();
            windows.push(
                div()
                    .id(SharedString::from(format!("compose-{}", m.name)))
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .w(px(m.width as f32 * k - GAP))
                    .h(px(m.height as f32 * k - GAP))
                    .overflow_hidden()
                    .when_some(image, |d, path| d.child(picture(path, l - left, t - top, w, h)))
                    .when_some(current, |d, path| {
                        let (cw, ch) = (m.width as f32 * k - GAP, m.height as f32 * k - GAP);
                        d.child(super::boxed(img(path), cw, ch).object_fit(ObjectFit::Cover).opacity(0.35))
                    })
                    .child(div().absolute().inset_0().border_2().border_color(if is_active {
                        accent
                    } else {
                        hsla(0., 0., 1., if mode == ComposeMode::Span { 0.35 } else { 0.25 })
                    }))
                    .child(
                        div()
                            .absolute()
                            .top_1()
                            .left_1()
                            .px_1p5()
                            .rounded_md()
                            .bg(if is_active { accent } else { hsla(0., 0., 0., 0.6) })
                            .text_xs()
                            .text_color(white())
                            .child(m.name.clone()),
                    )
                    .when(mode != ComposeMode::Span, |d| {
                        d.cursor_pointer().on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                if let Some(edit) = &mut this.span_edit {
                                    edit.active = name.clone();
                                }
                                cx.notify();
                            }),
                        )
                    })
                    .into_any_element(),
            );
        }

        let dragging = self.span_edit.as_ref().is_some_and(|e| e.drag.is_some());
        let stage = div()
            .id("span-stage")
            .relative()
            .w(px(stage_w))
            .h(px(stage_h))
            .overflow_hidden()
            .rounded_xl()
            .bg(hsla(0., 0., 0., 0.5))
            .cursor(if dragging { CursorStyle::ClosedHand } else { CursorStyle::OpenHand })
            .children(backdrop)
            .children(windows)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                    if ev.click_count >= 2 {
                        this.compose(cx, |f| *f = Framing::default());
                        return;
                    }
                    if let Some(edit) = &mut this.span_edit {
                        edit.drag = Some(ev.position);
                    }
                }),
            )
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                if ev.pressed_button == Some(MouseButton::Left) {
                    this.span_drag(ev.position, cx);
                } else if let Some(edit) = &mut this.span_edit {
                    edit.drag = None;
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if let Some(edit) = &mut this.span_edit {
                        edit.drag = None;
                    }
                    cx.notify();
                }),
            )
            .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, _, cx| {
                let dy = f32::from(ev.delta.pixel_delta(px(20.)).y);
                this.compose(cx, |f| f.zoom = (f.zoom * (1. + dy * 0.004)).clamp(1., MAX_ZOOM));
            }));

        // ---- the tools, acting on the screen (or span) being framed ----
        let framing = self.span_edit.as_ref()?.framing();
        let tool =
            |id: &'static str, name: &'static str, tip: &'static str| kit::Btn::new(id).icon(name).ghost().tooltip(tip);
        let mode_seg = |id: &'static str, label: &'static str, m: ComposeMode| {
            segment(id, label, mode == m, p).px_3().on_click(cx.listener(move |this, _, _, cx| {
                if let Some(edit) = &mut this.span_edit {
                    edit.mode = m;
                }
                cx.notify();
            }))
        };
        let modes = (monitors.len() > 1).then(|| {
            segmented_track(p)
                .child(mode_seg("compose-every", "Every screen", ComposeMode::Every))
                .child(mode_seg("compose-one", "One screen", ComposeMode::One))
                .child(mode_seg("compose-span", "Span", ComposeMode::Span))
        });
        // Desktop, lock screen or both: the same choice as everywhere else.
        let target_now = self.target;
        let target_seg = |id: &'static str, label: &'static str, t: super::TargetMode| {
            segment(id, label, target_now == t, p).px_2p5().on_click(cx.listener(move |this, _, _, cx| {
                this.target = t;
                cx.notify();
            }))
        };
        let surfaces = self.backend.lock().is_some().then(|| {
            segmented_track(p)
                .child(target_seg("compose-desktop", "Desktop", super::TargetMode::Desktop))
                .child(target_seg("compose-lock", "Lock", super::TargetMode::Lock))
                .child(target_seg("compose-both", "Both", super::TargetMode::Both))
        });
        let exit = self.span_edit.as_ref().map_or(ComposeExit::Set, |e| e.exit);
        let set_label = match mode {
            _ if exit == ComposeExit::Plan => "Done".to_owned(),
            ComposeMode::Span => "Span it".to_owned(),
            ComposeMode::One => format!("Set on {active}"),
            ComposeMode::Every => {
                format!("Set on {} screen{}", monitors.len(), if monitors.len() == 1 { "" } else { "s" })
            }
        };
        let tools = h_flex()
            .gap_0p5()
            .child(
                tool("compose-ccw", "rotate-ccw", "Turn left")
                    .on_click(cx.listener(|this, _, _, cx| this.compose(cx, |f| f.orient = f.orient.rotated_ccw()))),
            )
            .child(
                tool("compose-cw", "rotate-cw", "Turn right")
                    .on_click(cx.listener(|this, _, _, cx| this.compose(cx, |f| f.orient = f.orient.rotated_cw()))),
            )
            .child(
                tool("compose-mirror", "flip-horizontal-2", "Mirror")
                    .on_click(cx.listener(|this, _, _, cx| this.compose(cx, |f| f.orient = f.orient.mirrored()))),
            )
            .child(
                tool("compose-flip", "flip-vertical-2", "Flip upside down")
                    .on_click(cx.listener(|this, _, _, cx| this.compose(cx, |f| f.orient = f.orient.flipped()))),
            )
            .child(div().w(px(1.)).h(px(18.)).mx_1().bg(p.border_strong))
            .child(
                tool("compose-out", "zoom-out", "Zoom out")
                    .on_click(cx.listener(|this, _, _, cx| this.compose(cx, |f| f.zoom = (f.zoom / 1.15).max(1.)))),
            )
            .child(
                div()
                    .w(px(44.))
                    .text_xs()
                    .text_color(p.muted)
                    .flex()
                    .justify_center()
                    .child(format!("{:.0}%", framing.zoom * 100.)),
            )
            .child(
                tool("compose-in", "zoom-in", "Zoom in").on_click(
                    cx.listener(|this, _, _, cx| this.compose(cx, |f| f.zoom = (f.zoom * 1.15).min(MAX_ZOOM))),
                ),
            )
            .child(
                tool("compose-reset", "undo-2", "Reset")
                    .on_click(cx.listener(|this, _, _, cx| this.compose(cx, |f| *f = Framing::default()))),
            )
            .when(mode == ComposeMode::Every && monitors.len() > 1, |d| {
                d.child(tool("compose-all", "copy", "Use this framing on every screen").on_click(cx.listener(
                    |this, _, _, cx| {
                        let names: Vec<String> = this.monitors.iter().map(|m| m.name.clone()).collect();
                        if let Some(edit) = &mut this.span_edit {
                            let f = edit.framing();
                            for n in names {
                                edit.screens.insert(n, f);
                            }
                        }
                        cx.notify();
                    },
                )))
            });

        let hint = match mode {
            ComposeMode::Every => {
                format!("Framing {active}  ·  click a screen to frame it  ·  drag  ·  scroll to zoom")
            }
            ComposeMode::One => {
                format!("Only {active} changes  ·  click another screen to pick it  ·  drag  ·  scroll")
            }
            ComposeMode::Span => "Drag to move  ·  scroll to zoom  ·  double-click to reset".to_owned(),
        };
        let card = v_flex()
            .id("span-card")
            .rounded_xl()
            .bg(p.surface)
            .border_1()
            .border_color(p.border_strong)
            .shadow_lg()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                h_flex()
                    .gap_2p5()
                    .px_6()
                    .h(px(56.))
                    .border_b_1()
                    .border_color(p.border)
                    .child(icon("crop").size_4().text_color(accent))
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Compose"))
                    .children(modes)
                    .child(div().flex_1().text_xs().text_color(p.dim).child(hint))
                    .child(icon_button("span-close", "close", p).on_click(cx.listener(|this, _, _, cx| {
                        this.close_composer(cx);
                    }))),
            )
            .child(div().p_6().child(stage))
            .child(
                h_flex().justify_between().px_6().h(px(64.)).border_t_1().border_color(p.border).child(tools).child(
                    h_flex()
                        .gap_3()
                        .children(surfaces)
                        .child(button("span-cancel", "Cancel", BtnKind::Secondary, p).on_click(cx.listener(
                            |this, _, _, cx| {
                                this.close_composer(cx);
                            },
                        )))
                        .child(
                            button("span-commit", set_label, BtnKind::Primary, p)
                                .on_click(cx.listener(|this, _, _, cx| this.commit_span(cx))),
                        ),
                ),
            );

        Some(
            div()
                .id("span-overlay")
                .occlude()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(with_alpha(p.bg, 0.8))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.close_composer(cx);
                    }),
                )
                .child(card)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::dragged;
    use crate::span::{Framing, Orient};

    #[test]
    fn dragging_moves_within_the_spare_picture() {
        // A 2:1 picture on a 1:1 box has spare width only.
        let f = dragged(Framing::default(), (2000, 1000), 100., 100., 1000., 1000.);
        assert_eq!(f.pan.1, 0.);
        assert!((f.pan.0 - -1.).abs() < 1e-6, "dragged fully right shows the left edge");
    }

    #[test]
    fn orientations_compose() {
        let o = Orient::default();
        assert_eq!(o.rotated_cw().rotated_ccw(), o);
        assert_eq!(o.flipped().flipped(), o);
        assert_eq!(o.mirrored().mirrored(), o);
        assert_eq!(o.rotated_cw().dims((1920, 1080)), (1080, 1920));
    }
}
