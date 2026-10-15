//! The lightbox: the wallpaper floating in its own blurred, backlit glow,
//! with a filmstrip, a glass details panel, a magnifying loupe (hold Shift),
//! an "on your desks" mockup of your monitors, and slide/zoom transitions.

use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    FullPhase, Item, Muralist, Placement, Tab, boxed, develop, develop_bands, fade_in, file_name, glass, gridmodes,
    headers, icon, kit, local_key, mib, pal, progress_line, purity_color, radius, segment, segmented_track, with_alpha,
};
use crate::span::{self, Framing};

const TOP: f32 = 96.;
const FILM_H: f32 = 104.;
const SIDE: f32 = 84.;
const PANEL_W: f32 = 340.;
/// The shutter rail down the right edge.
const RAIL_W: f32 = 88.;
const RAIL_RIGHT: f32 = 20.;
const LOUPE: f32 = 260.;
const IMG_R: f32 = 10.;

/// Fit an `aspect` (w/h) rectangle inside `(x, y, w, h)`, centered.
fn fit_rect(aspect: f32, x: f32, y: f32, w: f32, h: f32) -> (f32, f32, f32, f32) {
    let fw = w.min(h * aspect);
    let fh = fw / aspect;
    (x + (w - fw) / 2., y + (h - fh) / 2., fw, fh)
}

impl Muralist {
    pub(super) fn open_lightbox(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.is_none() {
            return;
        }
        self.lightbox = true;
        self.lb_zoom = false;
        self.lb_dir = 0;
        self.focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn close_lightbox(&mut self, cx: &mut Context<Self>) {
        self.lightbox = false;
        self.lb_drag = None;
        self.lb_loupe = false;
        cx.notify();
    }

    pub(super) fn lightbox_step(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let items = self.current_items();
        let Some(ix) = self.selected_index(&items) else {
            return;
        };
        let next = (ix as isize + delta).clamp(0, items.len() as isize - 1) as usize;
        self.lightbox_jump(next, window, cx);
    }

    fn lightbox_jump(&mut self, next: usize, window: &mut Window, cx: &mut Context<Self>) {
        let items = self.current_items();
        let Some(ix) = self.selected_index(&items) else {
            return;
        };
        if next != ix && next < items.len() {
            self.lb_dir = if next > ix { 1 } else { -1 };
            self.select(items[next].clone(), window, cx);
        }
        // Keep paging in so browsing never hits a wall.
        if self.tab == Tab::Discover && next + 4 >= items.len() {
            self.search(false, cx);
        }
    }

    pub(super) fn toggle_zoom(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.enter_zoom(None, cx);
    }

    /// Into the zoom explorer at 100%, centered on `focus` (0..1 of the
    /// image) or the middle; out again if already there.
    fn enter_zoom(&mut self, focus: Option<(f32, f32)>, cx: &mut Context<Self>) {
        self.lb_zoom = !self.lb_zoom;
        if self.lb_zoom {
            let (iw, ih) = self.selected_dims.map_or((1., 1.), |(w, h)| (w as f32, h as f32));
            let (u, v) = focus.unwrap_or((0.5, 0.5));
            self.lb_z = 1. / self.scale;
            self.lb_pan = self.clamp_pan((u * iw, v * ih), self.lb_z);
        }
        self.lb_drag = None;
        cx.notify();
    }

    pub(super) fn lightbox_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        match ev.keystroke.key.as_str() {
            "escape" if self.lb_keys => {
                self.lb_keys = false;
                cx.notify();
            }
            "escape" => self.close_lightbox(cx),
            "?" => {
                self.lb_keys = !self.lb_keys;
                cx.notify();
            }
            "/" if ev.keystroke.modifiers.shift => {
                self.lb_keys = !self.lb_keys;
                cx.notify();
            }
            "left" | "h" => self.lightbox_step(-1, window, cx),
            "right" | "l" => self.lightbox_step(1, window, cx),
            "z" => self.toggle_zoom(window, cx),
            "0" if self.lb_zoom => {
                let z = self.fit_zoom();
                self.zoom_at(z, None, cx);
            }
            "1" if self.lb_zoom => {
                let z = 1. / self.scale;
                self.zoom_at(z, None, cx);
            }
            "+" | "=" if self.lb_zoom => {
                let z = self.lb_z * 1.25;
                self.zoom_at(z, None, cx);
            }
            "-" if self.lb_zoom => {
                let z = self.lb_z / 1.25;
                self.zoom_at(z, None, cx);
            }
            "m" => {
                self.lb_desks = !self.lb_desks;
                cx.notify();
            }
            "i" => {
                self.lb_panel = !self.lb_panel;
                cx.notify();
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    pub(super) fn render_lightbox(&mut self, _window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let p = pal();
        let Some(item) = self.selected.clone() else {
            return div().into_any_element();
        };
        let key = match &item {
            Item::Remote(w) => w.id.clone(),
            Item::Local(path) => local_key(path),
        };
        let (cw, ch) = self.content;
        let dims = self.selected_dims;
        let aspect = dims.map_or(16. / 9., |(w, h)| w as f32 / h.max(1) as f32);
        let zoom = self.lb_zoom && dims.is_some();
        let panel = self.lb_panel && !zoom && cw > 1100.;

        // Stage: the space the image gets.
        let stage_x = SIDE;
        let stage_y = TOP;
        // The rail takes the right edge; the info drawer opens beside it.
        let stage_w = (cw - SIDE * 2. - RAIL_W - RAIL_RIGHT - if panel { PANEL_W + 12. } else { 0. }).max(200.);
        let stage_h = (ch - TOP - FILM_H - 20.).max(160.);

        // Sources.
        let (base, full) = match &item {
            Item::Remote(w) => (self.remote_thumb(w, true), self.remote_full(w, cx)),
            Item::Local(path) => (self.local_thumb(path), Some(path.clone())),
        };
        let phase = self.full_phase(&item);
        let bands = match &phase {
            FullPhase::Loading(b) => Some(*b),
            _ => None,
        };
        let developing = bands.is_some();
        let sc = self.scale;
        let bucket = move |v: f32| ((v * sc / 256.).ceil() * 256.) as u32;
        let fit = match (&full, zoom) {
            (Some(f), false) => self.fitted(f, bucket(stage_w), bucket(stage_h)),
            _ => None,
        };
        let preview_fit = full.as_deref().and_then(|f| {
            let w = self.fit_width();
            let h = (self.preview_h() * self.scale).round() as u32;
            self.fitted(f, w, h)
        });
        let under = if developing { base.clone() } else { preview_fit.or_else(|| full.clone()).or(base.clone()) };
        let best = fit.clone().or(under.clone());

        let colors = self.selected_colors();
        let glow_c = glass::glow_color(&colors).unwrap_or(p.accent);

        // Neighbours for instant browsing.
        let items = self.current_items();
        let ix = self.selected_index(&items);
        if let (Some(i), false) = (ix, developing) {
            self.prefetch_item(items.get(i + 1), cx);
            if i > 0 {
                self.prefetch_item(items.get(i - 1), cx);
            }
        }

        // ---- the image (or the desks mockup) ----
        let stage: AnyElement = if zoom {
            let screen_fit = full.as_deref().and_then(|f| self.fitted(f, bucket(cw), bucket(ch)));
            self.render_lb_zoom(dims.unwrap(), full.clone(), base, screen_fit, glow_c, cx)
        } else if self.lb_desks && !self.monitors.is_empty() {
            self.render_lb_desks(best, dims, glow_c, stage_x, stage_y, stage_w, stage_h, &key)
        } else {
            let (x, y, w, h) = fit_rect(aspect, stage_x, stage_y, stage_w, stage_h);
            let dir = f32::from(self.lb_dir);
            let lb_bands = match (bands, &base) {
                (Some(b), Some(t)) => Some((b, t.clone())),
                _ => None,
            };
            let band_key = format!("lb-{key}");
            let rect = self.lb_rect.clone();
            let loupe = self.render_loupe(dims, full.clone().or(best), x, y, w, h);
            // While the original downloads, the camera hunts for focus over
            // it; when it lands, it locks.
            let hud = bands.map(|b| develop::hud(&band_key, b, &colors, glow_c, h)).unwrap_or_default();
            if developing {
                self.lb_develop_watch = Some(key.clone());
            }
            let lock = match (&phase, &item) {
                (FullPhase::Done, Item::Remote(w)) if self.lb_develop_watch.as_deref() == Some(key.as_str()) => {
                    develop::lock(&band_key, glow_c, format!("LOCKED  {}  ·  {:.1} MB", w.resolution, mib(w.file_size)))
                }
                _ => Vec::new(),
            };
            // At rest, just the viewfinder's corners around the picture.
            let viewfinder = (!developing).then(|| {
                div()
                    .absolute()
                    .left(px(x - 10.))
                    .top(px(y - 10.))
                    .w(px(w + 20.))
                    .h(px(h + 20.))
                    .children((0..4).map(|i| develop::bracket(i, 0., with_alpha(glow_c, 0.7))))
                    .with_animation(SharedString::from(format!("lb-vf-{key}")), glass::delayed(220, 420), |d, t| {
                        d.opacity(glass::ease_out_cubic(t))
                    })
            });

            let image = div()
                .absolute()
                .left(px(x))
                .top(px(y))
                .w(px(w))
                .h(px(h))
                .rounded(px(IMG_R))
                .shadow(glass::ambilight(glow_c, 60., 0.28))
                .when_some(under, |d, t| {
                    d.child(radius(
                        img(t)
                            .absolute()
                            .top_0()
                            .left_0()
                            .size_full()
                            .object_fit(ObjectFit::Fill)
                            .grayscale(developing)
                            .opacity(if developing { 0.6 } else { 1. }),
                        IMG_R,
                    ))
                })
                .when_some(lb_bands, |d, (b, t)| d.children(develop_bands(&band_key, &t, b, IMG_R, (w, h))))
                .when_some(fit, |d, f| {
                    d.child(fade_in(
                        SharedString::from(format!("lb-fade-{key}")),
                        radius(img(f).size_full().object_fit(ObjectFit::Fill), IMG_R),
                    ))
                })
                .children(loupe)
                .children(hud)
                .children(lock)
                // Opening grows in; browsing slides in from the travel direction.
                .with_animation(
                    SharedString::from(format!("lb-in-{key}-{}", self.lb_dir)),
                    Animation::new(Duration::from_millis(340)).with_easing(ease_out_quint()),
                    move |d, t| {
                        if dir == 0. {
                            let s = 0.9 + 0.1 * t;
                            let (sw, sh) = (w * s, h * s);
                            d.left(px(x + (w - sw) / 2.)).top(px(y + (h - sh) / 2.)).w(px(sw)).h(px(sh)).opacity(t)
                        } else {
                            d.left(px(x + dir * 70. * (1. - t))).opacity(t)
                        }
                    },
                );
            div()
                .on_children_prepainted(move |rects, _, _| rect.set(rects.first().copied()))
                .id("lb-stage")
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .cursor_pointer()
                .children(viewfinder)
                .child(image)
                .on_click(cx.listener(|this, ev: &ClickEvent, _, cx| {
                    if this.lb_loupe {
                        return;
                    }
                    // Zoom into the spot that was clicked.
                    let focus = this.lb_rect.get().map(|r| {
                        let pos = ev.position();
                        (
                            (f32::from(pos.x - r.origin.x) / f32::from(r.size.width)).clamp(0., 1.),
                            (f32::from(pos.y - r.origin.y) / f32::from(r.size.height)).clamp(0., 1.),
                        )
                    });
                    this.enter_zoom(focus, cx);
                }))
                .into_any_element()
        };

        // ---- chrome ----
        let backdrop = self.backdrop();
        let pill = |d: Div| {
            d.h(px(40.))
                .px_2()
                .gap_1()
                .rounded_full()
                .bg(hsla(0., 0., 0.05, 0.62))
                .border_1()
                .border_color(hsla(0., 0., 1., 0.09))
                .shadow_lg()
        };
        let chip = |text: String| {
            div()
                .px_2()
                .py_0p5()
                .rounded_full()
                .bg(hsla(0., 0., 1., 0.08))
                .text_xs()
                .text_color(hsla(0., 0., 1., 0.75))
                .child(text)
        };
        let round_btn = |id: &'static str, name: &'static str, tip: &'static str| {
            kit::Btn::new(id).icon(name).ghost().sm().round().tooltip(tip).render()
        };

        let (title, purity, url) = match &item {
            Item::Remote(w) => (format!("wallhaven-{}", w.id), Some(w.purity), Some(w.url.clone())),
            Item::Local(path) => (file_name(path), None, None),
        };
        let more_pages = self.tab == Tab::Discover && self.page < self.last_page;
        let counter = ix.map(|i| format!("{} / {}{}", i + 1, items.len(), if more_pages { "+" } else { "" }));

        let decoding = title.clone();
        let left_pill = pill(h_flex())
            .pl_4()
            .child(
                v_flex()
                    .mr_1()
                    .child(div().font_weight(FontWeight::SEMIBOLD).text_color(white()).with_animation(
                        SharedString::from(format!("lb-decode-{key}")),
                        Animation::new(Duration::from_millis(650)),
                        move |d, t| d.child(headers::scrambled(&decoding, t)),
                    ))
                    // Its palette, drawn in under the name.
                    .child(h_flex().gap(px(2.)).children(colors.iter().take(6).enumerate().map(|(i, c)| {
                        div().h(px(3.)).rounded_full().bg(*c).with_animation(
                            SharedString::from(format!("lb-pal-{key}-{i}")),
                            glass::delayed(i as u64 * 60, 260),
                            |d, t| d.w(px(14. * glass::ease_out_cubic(t))),
                        )
                    }))),
            )
            .children(dims.map(|(w, h)| chip(format!("{w}x{h}"))))
            .when_some(purity, |d, purity| {
                let c = purity_color(purity);
                d.child(
                    h_flex()
                        .gap_1()
                        .px_2()
                        .py_0p5()
                        .rounded_full()
                        .bg(with_alpha(c, 0.18))
                        .text_xs()
                        .text_color(c)
                        .child(div().size(px(6.)).rounded_full().bg(c))
                        .child(purity.label()),
                )
            })
            .children(counter.map(chip));

        let desks = self.lb_desks;
        let right_pill = pill(h_flex())
            .child(
                segmented_track(p)
                    .bg(hsla(0., 0., 0., 0.3))
                    .border_color(hsla(0., 0., 1., 0.06))
                    .child(segment("lb-mode-img", "Image", !desks && !zoom, p).on_click(cx.listener(
                        |this, _, _, cx| {
                            this.lb_desks = false;
                            this.lb_zoom = false;
                            cx.notify();
                        },
                    )))
                    .child(segment("lb-mode-desk", "On your desks", desks && !zoom, p).on_click(cx.listener(
                        |this, _, _, cx| {
                            this.lb_desks = true;
                            this.lb_zoom = false;
                            cx.notify();
                        },
                    )))
                    .child(segment("lb-mode-zoom", "Zoom", zoom, p).on_click(cx.listener(|this, _, window, cx| {
                        if !this.lb_zoom {
                            this.toggle_zoom(window, cx);
                        }
                    }))),
            )
            .child(round_btn("lb-keys", "info", "Shortcuts (?)").on_click(cx.listener(|this, _, _, cx| {
                this.lb_keys = !this.lb_keys;
                cx.notify();
            })))
            .child(round_btn("lb-panel", "panel-right", "Details panel (I)").on_click(cx.listener(|this, _, _, cx| {
                this.lb_panel = !this.lb_panel;
                cx.notify();
            })))
            .child(
                round_btn("lb-close", "close", "Close (Esc)")
                    .on_click(cx.listener(|this, _, _, cx| this.close_lightbox(cx))),
            );

        let top = h_flex()
            .id("lb-top")
            .occlude()
            .absolute()
            .top_4()
            .left(px(20.))
            .right(px(20.))
            .justify_between()
            .child(left_pill)
            .child(right_pill);

        let has_prev = ix.is_some_and(|i| i > 0);
        let has_next = ix.is_some_and(|i| i + 1 < items.len()) || more_pages;
        // Each arrow peeks the wallpaper it leads to while hovered.
        let thumb_at = |this: &mut Self, j: Option<usize>| {
            let item = j.and_then(|j| items.get(j))?;
            match item {
                Item::Remote(w) => this.remote_thumb(w, false),
                Item::Local(path) => this.local_thumb(path),
            }
        };
        let prev_thumb = thumb_at(self, ix.and_then(|i| i.checked_sub(1)));
        let next_thumb = thumb_at(self, ix.map(|i| i + 1));
        let arrow = |id: &'static str, name: &'static str, left: bool, peek: Option<std::path::PathBuf>| {
            kit::Btn::new(id)
                .icon(name)
                .lg()
                .round()
                .render()
                .group(id)
                .occlude()
                .absolute()
                .top(px(TOP + stage_h / 2. - 21.))
                .map(|d| if left { d.left(px(22.)) } else { d.left(px(stage_x + stage_w + 22.)) })
                .when_some(peek, |d, t| {
                    let (pw, ph) = (150., 94.);
                    d.child(
                        div()
                            .absolute()
                            .top(px(21. - ph / 2.))
                            .map(|d| if left { d.left(px(52.)) } else { d.right(px(52.)) })
                            .w(px(pw))
                            .h(px(ph))
                            .rounded_lg()
                            .bg(hsla(0., 0., 0., 0.6))
                            .border_2()
                            .border_color(with_alpha(glow_c, 0.8))
                            .shadow(glass::ambilight(glow_c, 24., 0.5))
                            .invisible()
                            .group_hover(id, |s| s.visible())
                            .child(
                                div()
                                    .absolute()
                                    .top(px(2.))
                                    .left(px(2.))
                                    .child(radius(boxed(img(t), pw - 4., ph - 4.).object_fit(ObjectFit::Cover), 6.)),
                            ),
                    )
                })
        };

        let filmstrip = (!zoom).then(|| self.render_filmstrip(&items, ix, glow_c, cx));
        let side_panel = panel.then(|| self.render_lb_info(&item, cx));
        let dock = (!zoom).then(|| self.render_lb_dock(&item, cx));
        let _ = (&colors, &url);
        let keys = self.lb_keys.then(|| self.render_lb_keys(cx));
        let progress = match phase {
            FullPhase::Loading(b) if zoom => Some(b.fraction),
            _ => None,
        };

        div()
            .image_cache(self.preview_cache.clone())
            .id("lightbox")
            .track_focus(&self.focus)
            .occlude()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .bg(black())
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| this.lightbox_key(ev, window, cx)))
            .on_modifiers_changed(cx.listener(|this, ev: &ModifiersChangedEvent, _, cx| {
                if this.lb_loupe != ev.modifiers.shift {
                    this.lb_loupe = ev.modifiers.shift;
                    cx.notify();
                }
            }))
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                this.lb_cursor = Some(ev.position);
                if this.lb_loupe {
                    cx.notify();
                }
            }))
            // Ambient mode: the picture's own blurred light fills the room.
            .when_some(backdrop, |d, (path, bkey)| {
                d.child(fade_in(
                    SharedString::from(format!("lb-amb-{bkey}")),
                    img(path).size_full().object_fit(ObjectFit::Cover),
                ))
            })
            .child(div().absolute().top_0().left_0().size_full().bg(hsla(0., 0., 0., 0.5)))
            // Slow light drifting around the room in the picture's colors.
            .children(colors.iter().take(2).enumerate().map(|(k, c)| {
                let c = Hsla { s: c.s.max(0.4), l: c.l.clamp(0.35, 0.65), a: 1., ..*c };
                let phase = k as f32 * std::f32::consts::PI;
                div().absolute().size(px(4.)).rounded_full().shadow(glass::ambilight(c, 420., 0.22)).with_animation(
                    SharedString::from(format!("lb-drift-{k}")),
                    Animation::new(Duration::from_secs(26)).repeat(),
                    move |d, t| {
                        let a = t * std::f32::consts::TAU + phase;
                        d.left(px(cw / 2. + a.cos() * cw * 0.32)).top(px(ch / 2. + (a * 1.3).sin() * ch * 0.26))
                    },
                )
            }))
            .child(div().absolute().top_0().left_0().right_0().h(px(200.)).bg(linear_gradient(
                180.,
                linear_color_stop(hsla(0., 0., 0., 0.55), 0.),
                linear_color_stop(hsla(0., 0., 0., 0.), 1.),
            )))
            .child(div().absolute().bottom_0().left_0().right_0().h(px(220.)).bg(linear_gradient(
                0.,
                linear_color_stop(hsla(0., 0., 0., 0.65), 0.),
                linear_color_stop(hsla(0., 0., 0., 0.), 1.),
            )))
            .child(stage)
            .children(filmstrip)
            .children(dock)
            .children(side_panel)
            .child(top)
            .children(keys)
            .when_some(progress, |d, fraction| {
                d.child(div().absolute().top_0().left_0().right_0().child(progress_line(
                    SharedString::from(format!("lb-sweep-{key}")),
                    fraction,
                    2.,
                    p,
                )))
            })
            .when(has_prev && !zoom, |d| {
                d.child(
                    arrow("lb-prev", "chevron-left", true, prev_thumb.clone())
                        .on_click(cx.listener(|this, _, window, cx| this.lightbox_step(-1, window, cx))),
                )
            })
            .when(has_next && !zoom, |d| {
                d.child(
                    arrow("lb-next", "chevron-right", false, next_thumb.clone())
                        .on_click(cx.listener(|this, _, window, cx| this.lightbox_step(1, window, cx))),
                )
            })
            .into_any_element()
    }

    /// Magnifier at the pointer showing the original's real pixels.
    fn render_loupe(
        &self,
        dims: Option<(u32, u32)>,
        src: Option<PathBuf>,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) -> Option<AnyElement> {
        if !self.lb_loupe {
            return None;
        }
        let ((iw, ih), src, cursor, rect) = (dims?, src?, self.lb_cursor?, self.lb_rect.get()?);
        let local_x = f32::from(cursor.x - rect.origin.x);
        let local_y = f32::from(cursor.y - rect.origin.y);
        if local_x < 0. || local_y < 0. || local_x > w || local_y > h {
            return None;
        }
        let _ = (x, y);
        // One image pixel per physical screen pixel.
        let (lw, lh) = (iw as f32 / self.scale, ih as f32 / self.scale);
        let (u, v) = (local_x / w, local_y / h);
        let half = LOUPE / 2.;
        Some(
            div()
                .absolute()
                .left(px(local_x - half))
                .top(px(local_y - half))
                .w(px(LOUPE))
                .h(px(LOUPE))
                .overflow_hidden()
                .rounded(px(14.))
                .border_2()
                .border_color(hsla(0., 0., 1., 0.85))
                .shadow_lg()
                .bg(black())
                .child(
                    img(src)
                        .absolute()
                        .left(px(half - u * lw))
                        .top(px(half - v * lh))
                        .w(px(lw))
                        .h(px(lh))
                        .object_fit(ObjectFit::Fill),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(half - 3.))
                        .top(px(half - 3.))
                        .size(px(6.))
                        .rounded_full()
                        .border_1()
                        .border_color(white()),
                )
                .child(
                    div()
                        .absolute()
                        .bottom_2()
                        .right_2()
                        .px_1p5()
                        .rounded_md()
                        .bg(hsla(0., 0., 0., 0.6))
                        .text_xs()
                        .text_color(white())
                        .child("1:1"),
                )
                .into_any_element(),
        )
    }

    /// The picture on a mockup of your monitors, bezels and all.
    #[allow(clippy::too_many_arguments)]
    fn render_lb_desks(
        &mut self,
        src: Option<PathBuf>,
        dims: Option<(u32, u32)>,
        _glow_c: Hsla,
        sx: f32,
        sy: f32,
        sw: f32,
        sh: f32,
        key: &str,
    ) -> AnyElement {
        let (min_x, min_y, max_x, max_y) = crate::monitors::extent(&self.monitors);
        let (bw, bh) = ((max_x - min_x) as f32, (max_y - min_y) as f32);
        // Leave room for stands under the screens.
        let k = (sw / bw).min((sh - 40.) / bh);
        let (ox, oy) = (sx + (sw - bw * k) / 2., sy + (sh - 40. - bh * k) / 2.);
        let bezel = 10.;
        let (iw, ih) = dims.unwrap_or((16, 9));
        let span = self.monitors.len() > 1 && iw as f32 / ih.max(1) as f32 >= 2.4;
        let (s, off_x, off_y) = span::layout(iw, ih, f64::from(bw), f64::from(bh), Framing::default());
        let (img_w, img_h) = ((f64::from(iw) * s) as f32 * k, (f64::from(ih) * s) as f32 * k);

        let screens: Vec<AnyElement> = self
            .monitors
            .clone()
            .into_iter()
            .enumerate()
            .map(|(i, m)| {
                let left = ox + (m.x - min_x) as f32 * k;
                let top = oy + (m.y - min_y) as f32 * k;
                let (w, h) = (m.width as f32 * k, m.height as f32 * k);
                let screen = div()
                    .absolute()
                    .left(px(bezel))
                    .top(px(bezel))
                    .w(px(w - bezel * 2.))
                    .h(px(h - bezel * 2.))
                    .overflow_hidden()
                    .bg(black())
                    .when_some(src.clone(), |d, path| {
                        if span {
                            // One picture across all screens.
                            let il = -(off_x as f32) * k - (left - ox) - bezel;
                            let it = -(off_y as f32) * k - (top - oy) - bezel;
                            d.child(
                                img(path)
                                    .absolute()
                                    .left(px(il))
                                    .top(px(it))
                                    .w(px(img_w))
                                    .h(px(img_h))
                                    .object_fit(ObjectFit::Fill),
                            )
                        } else {
                            d.child(img(path).size_full().object_fit(ObjectFit::Cover))
                        }
                    });
                div()
                    .absolute()
                    .left(px(left + 4.))
                    .top(px(top))
                    .w(px(w - 8.))
                    .h(px(h + 34.))
                    .child(
                        // Bezel
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .w_full()
                            .h(px(h))
                            .rounded(px(8.))
                            .bg(hsla(0., 0., 0.06, 1.))
                            .border_1()
                            .border_color(hsla(0., 0., 1., 0.12))
                            .shadow_lg()
                            .child(screen),
                    )
                    .child(
                        // Stand
                        div()
                            .absolute()
                            .top(px(h))
                            .left(px((w - 8.) / 2. - 14.))
                            .w(px(28.))
                            .h(px(22.))
                            .bg(hsla(0., 0., 0.12, 1.)),
                    )
                    .child(
                        div()
                            .absolute()
                            .top(px(h + 22.))
                            .left(px((w - 8.) / 2. - 46.))
                            .w(px(92.))
                            .h(px(8.))
                            .rounded_full()
                            .bg(hsla(0., 0., 0.14, 1.)),
                    )
                    .with_animation(
                        SharedString::from(format!("desk-{key}-{i}")),
                        Animation::new(Duration::from_millis(380 + i as u64 * 90)).with_easing(move |t| {
                            ((t * (0.38 + i as f32 * 0.09) - i as f32 * 0.09) / 0.38).clamp(0., 1.)
                        }),
                        move |d, t| {
                            let e = super::glass::ease_out_cubic(t);
                            d.top(px(top + 30. * (1. - e))).opacity(e)
                        },
                    )
                    .into_any_element()
            })
            .collect();
        div().absolute().top_0().left_0().size_full().children(screens).into_any_element()
    }

    /// Zoom explorer: scroll to zoom at the cursor, drag to pan, a minimap
    /// to jump around, double-click between fit and 100%.
    #[allow(clippy::too_many_arguments)]
    fn render_lb_zoom(
        &mut self,
        (iw, ih): (u32, u32),
        full: Option<PathBuf>,
        base: Option<PathBuf>,
        fit: Option<PathBuf>,
        glow_c: Hsla,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (cw, ch) = self.content;
        let (iwf, ihf) = (iw as f32, ih as f32);
        let z = self.lb_z;
        let (px_, py_) = self.lb_pan;
        let (img_w, img_h) = (iwf * z, ihf * z);
        let left = cw / 2. - px_ * z;
        let top = ch / 2. - py_ * z;
        // Past ~50% the original is sharper than the screen-sized copy.
        let src = if z * self.scale >= 0.5 { full.or(fit.clone()) } else { fit.clone().or(full) };
        let percent = (z * self.scale * 100.).round() as u32;
        let dragging = self.lb_drag.is_some_and(|(_, moved)| moved);

        // Minimap with the visible region outlined.
        let mm_w = 220.;
        let mm_h = mm_w * ihf / iwf;
        let (mm_w, mm_h) = if mm_h > 160. { (160. * iwf / ihf, 160.) } else { (mm_w, mm_h) };
        let vis_x0 = ((px_ - cw / 2. / z) / iwf).clamp(0., 1.);
        let vis_y0 = ((py_ - ch / 2. / z) / ihf).clamp(0., 1.);
        let vis_x1 = ((px_ + cw / 2. / z) / iwf).clamp(0., 1.);
        let vis_y1 = ((py_ + ch / 2. / z) / ihf).clamp(0., 1.);
        let minimap = div()
            .id("lb-minimap")
            .occlude()
            .absolute()
            .right(px(20.))
            .bottom(px(20.))
            .w(px(mm_w))
            .h(px(mm_h))
            .rounded_lg()
            .overflow_hidden()
            .border_1()
            .border_color(hsla(0., 0., 1., 0.2))
            .shadow_lg()
            .bg(hsla(0., 0., 0., 0.6))
            .cursor_pointer()
            .when_some(fit.or(base.clone()), |d, t| {
                d.child(img(t).size_full().object_fit(ObjectFit::Fill).opacity(0.75))
            })
            .child(
                div()
                    .absolute()
                    .left(px(vis_x0 * mm_w))
                    .top(px(vis_y0 * mm_h))
                    .w(px(((vis_x1 - vis_x0) * mm_w).max(4.)))
                    .h(px(((vis_y1 - vis_y0) * mm_h).max(4.)))
                    .border_2()
                    .border_color(glow_c)
                    .bg(with_alpha(glow_c, 0.12)),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                    this.lb_mm_drag = true;
                    this.minimap_jump(ev.position, mm_w, mm_h, cx);
                }),
            )
            .on_mouse_move(cx.listener(move |this, ev: &MouseMoveEvent, _, cx| {
                if this.lb_mm_drag && ev.pressed_button == Some(MouseButton::Left) {
                    this.minimap_jump(ev.position, mm_w, mm_h, cx);
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, _| this.lb_mm_drag = false));

        let hud = h_flex().absolute().bottom(px(20.)).left_0().right_0().justify_center().child(
            h_flex()
                .gap_3()
                .px_4()
                .h(px(36.))
                .rounded_full()
                .bg(hsla(0., 0., 0.05, 0.7))
                .border_1()
                .border_color(hsla(0., 0., 1., 0.1))
                .text_xs()
                .text_color(hsla(0., 0., 1., 0.6))
                .child(
                    div().text_sm().font_weight(FontWeight::SEMIBOLD).text_color(white()).child(format!("{percent}%")),
                )
                .child("scroll to zoom  ·  drag to pan  ·  double-click fit / 100%  ·  0 fit  ·  1 actual"),
        );

        div()
            .id("lb-zoom")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .overflow_hidden()
            .cursor(if dragging { CursorStyle::ClosedHand } else { CursorStyle::OpenHand })
            .child(
                div()
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .w(px(img_w))
                    .h(px(img_h))
                    .shadow(glass::ambilight(glow_c, 60., 0.25))
                    .when_some(base, |d, t| {
                        d.child(img(t).absolute().top_0().left_0().size_full().object_fit(ObjectFit::Fill))
                    })
                    .when_some(src, |d, f| {
                        d.child(img(f).absolute().top_0().left_0().size_full().object_fit(ObjectFit::Fill))
                    }),
            )
            .child(hud)
            .child(minimap)
            .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, _, cx| {
                let dy = f32::from(ev.delta.pixel_delta(px(20.)).y);
                let factor = (dy * 0.0025).exp();
                this.zoom_at(this.lb_z * factor, Some(ev.position), cx);
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                    if ev.click_count >= 2 {
                        let actual = 1. / this.scale;
                        let target = if (this.lb_z - actual).abs() < 0.01 { this.fit_zoom() } else { actual };
                        this.zoom_at(target, Some(ev.position), cx);
                        return;
                    }
                    this.lb_drag = Some((ev.position, false));
                }),
            )
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                if this.lb_mm_drag {
                    return;
                }
                let Some((last, moved)) = this.lb_drag else { return };
                if ev.pressed_button != Some(MouseButton::Left) {
                    this.lb_drag = None;
                    return;
                }
                let delta = ev.position - last;
                if !moved && f32::from(delta.x).abs() + f32::from(delta.y).abs() < 3. {
                    return;
                }
                let z = this.lb_z;
                let pan = (this.lb_pan.0 - f32::from(delta.x) / z, this.lb_pan.1 - f32::from(delta.y) / z);
                this.lb_pan = this.clamp_pan(pan, z);
                this.lb_drag = Some((ev.position, true));
                cx.notify();
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.lb_drag = None;
                    cx.notify();
                }),
            )
            .into_any_element()
    }

    /// Zoom that shows the whole image in the window.
    fn fit_zoom(&self) -> f32 {
        let (iw, ih) = self.selected_dims.unwrap_or((1, 1));
        let (cw, ch) = self.content;
        (cw / iw as f32).min(ch / ih as f32)
    }

    /// Keep the image covering the view, or centered when it's smaller.
    fn clamp_pan(&self, (x, y): (f32, f32), z: f32) -> (f32, f32) {
        let (iw, ih) = self.selected_dims.map_or((1., 1.), |(w, h)| (w as f32, h as f32));
        let (cw, ch) = self.content;
        let axis = |v: f32, size: f32, view: f32| {
            let half = view / 2. / z;
            if half * 2. >= size { size / 2. } else { v.clamp(half, size - half) }
        };
        (axis(x, iw, cw), axis(y, ih, ch))
    }

    /// Change zoom keeping the image point under `at` (window coords) fixed.
    fn zoom_at(&mut self, z: f32, at: Option<Point<Pixels>>, cx: &mut Context<Self>) {
        let min = self.fit_zoom().min(1. / self.scale);
        let z = z.clamp(min, 3. / self.scale);
        let (cw, ch) = self.content;
        let (inset_x, inset_y) = ((self.viewport.0 - cw) / 2., (self.viewport.1 - ch) / 2.);
        let (ax, ay) = at.map_or((cw / 2., ch / 2.), |p| (f32::from(p.x) - inset_x, f32::from(p.y) - inset_y));
        let old = self.lb_z;
        // Image point under the anchor before, kept under it after.
        let ix = self.lb_pan.0 + (ax - cw / 2.) / old;
        let iy = self.lb_pan.1 + (ay - ch / 2.) / old;
        let pan = (ix - (ax - cw / 2.) / z, iy - (ay - ch / 2.) / z);
        self.lb_z = z;
        self.lb_pan = self.clamp_pan(pan, z);
        cx.notify();
    }

    fn minimap_jump(&mut self, pos: Point<Pixels>, mm_w: f32, mm_h: f32, cx: &mut Context<Self>) {
        let (cw, ch) = self.content;
        let (inset_x, inset_y) = ((self.viewport.0 - cw) / 2., (self.viewport.1 - ch) / 2.);
        // Minimap sits 20px from the content's bottom-right corner.
        let mm_left = cw - 20. - mm_w;
        let mm_top = ch - 20. - mm_h;
        let u = ((f32::from(pos.x) - inset_x - mm_left) / mm_w).clamp(0., 1.);
        let v = ((f32::from(pos.y) - inset_y - mm_top) / mm_h).clamp(0., 1.);
        let (iw, ih) = self.selected_dims.map_or((1., 1.), |(w, h)| (w as f32, h as f32));
        self.lb_pan = self.clamp_pan((u * iw, v * ih), self.lb_z);
        cx.notify();
    }

    /// Neighbouring wallpapers along the bottom; click to jump.
    fn render_filmstrip(
        &mut self,
        items: &[Item],
        ix: Option<usize>,
        glow_c: Hsla,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(ix) = ix else {
            return div().into_any_element();
        };
        let lo = ix.saturating_sub(6);
        let hi = (ix + 7).min(items.len());
        let frames: Vec<AnyElement> = (lo..hi)
            .map(|i| {
                let current = i == ix;
                let thumb = match &items[i] {
                    Item::Remote(w) => self.remote_thumb(w, false),
                    Item::Local(path) => self.local_thumb(path),
                };
                // Cover flow: frames shrink and fade with distance from the
                // current one, which stands lifted in its own light.
                let dist = (i as f32 - ix as f32).abs();
                let scale = (1. - dist * 0.13).max(0.5);
                let (w, h) = (124. * scale, 78. * scale);
                let own = gridmodes::first_color(&items[i], &self.local_palettes).map_or(glow_c, |c| Hsla {
                    s: c.s.max(0.45),
                    l: 0.6,
                    a: 1.,
                    ..c
                });
                let frame = div()
                    .id(("film", i))
                    .relative()
                    .flex_none()
                    .w(px(w))
                    .h(px(h))
                    .rounded_lg()
                    .bg(hsla(0., 0., 0., 0.4))
                    .cursor_pointer()
                    .when_some(thumb, |d, t| {
                        d.child(
                            div()
                                .absolute()
                                .top(px(2.))
                                .left(px(2.))
                                .child(radius(boxed(img(t), w - 4., h - 4.).object_fit(ObjectFit::Cover), 6.)),
                        )
                    })
                    .child(div().absolute().inset_0().rounded_lg().border_2().border_color(if current {
                        own
                    } else {
                        hsla(0., 0., 1., 0.07)
                    }))
                    .map(|d| {
                        if current {
                            d.shadow(glass::ambilight(own, 24., 0.75))
                        } else {
                            d.opacity((1. - dist * 0.11).max(0.3)).hover(move |s| s.opacity(1.))
                        }
                    })
                    .on_click(cx.listener(move |this, _, window, cx| this.lightbox_jump(i, window, cx)));
                // The current frame carries its place in the set beneath it.
                v_flex()
                    .items_center()
                    .gap_1()
                    .child(frame)
                    .when(current, |d| {
                        d.child(div().w(px(w * 0.6)).h(px(2.)).rounded_full().bg(own).with_animation(
                            SharedString::from(format!("film-mark-{i}")),
                            Animation::new(Duration::from_millis(320)).with_easing(ease_out_quint()),
                            move |d, t| d.w(px(w * 0.6 * t)),
                        ))
                    })
                    .into_any_element()
            })
            .collect();
        h_flex()
            .id("filmstrip")
            .occlude()
            .absolute()
            .bottom(px(14.))
            .left_0()
            .right_0()
            .h(px(FILM_H - 14.))
            .justify_center()
            .items_end()
            .pb_2()
            .gap_2()
            .children(frames)
            .into_any_element()
    }

    /// Floating glass dock over the image: where it goes and how.
    /// The shutter rail down the right edge, as on a camera: where it goes
    /// stacked above a big round shutter that sets it, the tools below.
    fn render_lb_dock(&mut self, item: &Item, cx: &mut Context<Self>) -> AnyElement {
        use super::TargetMode;
        let p = pal();
        let save_btn = self.gallery_button("lb-gallery", item, cx);
        let mode = self.target;
        let lock_ok = self.backend.lock().is_some();
        let c = glass::glow_color(&self.selected_colors()).unwrap_or(p.accent);
        let aiming = self.lb_set_hover;
        let screens = self.monitors.len();
        let set_item = item.clone();
        // More like this: the search that found the lookalikes (like: is
        // challenged by Wallhaven now).
        let like = match item {
            Item::Remote(w) => Some(self.similar_query.get(&w.id).cloned().unwrap_or_else(|| format!("like:{}", w.id))),
            Item::Local(_) => None,
        };
        let remote = matches!(item, Item::Remote(_));
        let info_open = self.lb_panel;
        let dim = hsla(0., 0., 1., 0.55);

        // ---- where it goes ----
        let target = |id: &'static str, icon_name: &'static str, label: &'static str, m: TargetMode| {
            let on = mode == m;
            v_flex()
                .id(id)
                .w(px(64.))
                .py_1p5()
                .items_center()
                .gap_1()
                .rounded_lg()
                .cursor_pointer()
                .bg(if on { with_alpha(c, 0.18) } else { gpui_kit::transparent_black() })
                .border_1()
                .border_color(if on { with_alpha(c, 0.55) } else { gpui_kit::transparent_black() })
                .hover(move |s| s.bg(hsla(0., 0., 1., 0.06)))
                .child(icon(icon_name).size_4().text_color(if on { c } else { dim }))
                .child(
                    div()
                        .text_size(px(10.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(if on { white() } else { dim })
                        .child(label),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.target = m;
                    cx.notify();
                }))
        };

        // ---- the shutter ----
        let ink = super::on_accent(c);
        const SHUTTER: f32 = 64.;
        let ring = if aiming { 6. } else { 4. };
        let shutter = div()
            .id("lb-set-all")
            .relative()
            .size(px(SHUTTER))
            .rounded_full()
            .cursor_pointer()
            .border_2()
            .border_color(with_alpha(c, if aiming { 1. } else { 0.7 }))
            .shadow(glass::ambilight(c, if aiming { 30. } else { 16. }, if aiming { 0.85 } else { 0.45 }))
            .child(
                div()
                    .absolute()
                    .top(px(ring))
                    .left(px(ring))
                    .size(px(SHUTTER - 4. - ring * 2.))
                    .rounded_full()
                    .bg(c)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("check").size(px(22.)).text_color(ink)),
            )
            .on_hover(cx.listener(|this, over: &bool, _, cx| {
                if this.lb_set_hover != *over {
                    this.lb_set_hover = *over;
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, _, cx| {
                let targets = this.target.targets();
                this.assign_item_then(set_item.clone(), targets, Placement::All, true, cx);
            }));
        let caption = v_flex()
            .items_center()
            .child(div().text_sm().font_weight(FontWeight::BOLD).text_color(white()).child("Set"))
            .child(
                div()
                    .text_size(px(10.))
                    .text_color(dim)
                    .child(format!("{screens} screen{}", if screens == 1 { "" } else { "s" })),
            );

        // ---- tools ----
        let tool =
            |id: &'static str, name: &'static str, tip: &'static str| kit::Btn::new(id).icon(name).ghost().tooltip(tip);
        let tools = v_flex()
            .items_center()
            .gap_1()
            .child(
                tool("lb-span", "frame", "Span across displays")
                    .on_click(cx.listener(|this, _, _, cx| this.open_span_editor(cx)))
                    .render(),
            )
            .child(save_btn.render())
            .when(remote, |d| {
                d.child(
                    tool("lb-dl", "arrow-down", "Save to library")
                        .on_click(cx.listener(|this, _, _, cx| this.download_selected(cx)))
                        .render(),
                )
            })
            .when_some(like, |d, like| {
                d.child(
                    tool("lb-like", "images", "More like this")
                        .on_click(cx.listener(move |this, _, window, cx| this.search_for(like.clone(), window, cx)))
                        .render(),
                )
            })
            .child(
                tool("lb-info", "info", "Info (I)")
                    .selected(info_open)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.lb_panel = !this.lb_panel;
                        cx.notify();
                    }))
                    .render(),
            );

        let rule = || div().w(px(40.)).h(px(1.)).bg(hsla(0., 0., 1., 0.1));
        // The glass hugs what's on it, centred beside the picture: no empty
        // rail above or below.
        let capsule = v_flex()
            .id("lb-rail")
            .occlude()
            .w_full()
            .py_4()
            .items_center()
            .gap_4()
            .rounded_2xl()
            .bg(hsla(0., 0., 0.04, 0.55))
            .border_1()
            .border_color(with_alpha(c, 0.2))
            .when(lock_ok, |d| {
                d.child(
                    v_flex()
                        .gap_1()
                        .child(target("lbd-desktop", "monitor", "Desktop", TargetMode::Desktop))
                        .child(target("lbd-lock", "lock", "Lock", TargetMode::Lock))
                        .child(target("lbd-both", "layers", "Both", TargetMode::Both)),
                )
                .child(rule())
            })
            .child(v_flex().items_center().gap_2().child(shutter).child(caption))
            .child(rule())
            .child(tools);
        v_flex()
            .absolute()
            .top(px(TOP))
            .bottom(px(FILM_H + 8.))
            .right(px(RAIL_RIGHT))
            .w(px(RAIL_W))
            .justify_center()
            .child(capsule)
            .into_any_element()
    }

    /// The Info drawer: the same Info view as the sidebar, on glass.
    fn render_lb_info(&mut self, item: &Item, cx: &mut Context<Self>) -> AnyElement {
        let p = pal();
        self.details_w = PANEL_W - 32.;
        let content = match item {
            Item::Remote(w) => {
                let w = w.clone();
                let c = glass::glow_color(&self.selected_colors()).unwrap_or(p.accent);
                self.render_case_info(&w, c, cx)
            }
            Item::Local(path) => {
                let path = path.clone();
                let palette = self.render_xray_palette(cx);
                v_flex()
                    .gap_5()
                    .children(palette)
                    .child(
                        super::section("File", p)
                            .child(div().text_xs().text_color(p.muted).child(path.display().to_string())),
                    )
                    .into_any_element()
            }
        };
        self.details_w = self.details_base();
        v_flex()
            .id("lb-info-drawer")
            .occlude()
            .absolute()
            .top(px(TOP))
            .bottom(px(FILM_H + 20.))
            .right(px(RAIL_RIGHT + RAIL_W + 12.))
            .w(px(PANEL_W))
            .rounded_xl()
            .bg(with_alpha(p.surface, 0.9))
            .border_1()
            .border_color(hsla(0., 0., 1., 0.09))
            .shadow_lg()
            .child(
                h_flex()
                    .flex_none()
                    .justify_between()
                    .px_4()
                    .h(px(48.))
                    .border_b_1()
                    .border_color(p.border)
                    .child(
                        h_flex()
                            .gap_2()
                            .child(icon("info").size_4().text_color(p.accent))
                            .child(div().font_weight(FontWeight::SEMIBOLD).child("Info")),
                    )
                    .child(
                        kit::Btn::new("lb-info-close")
                            .icon("close")
                            .ghost()
                            .sm()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.lb_panel = false;
                                cx.notify();
                            }))
                            .render(),
                    ),
            )
            .child(div().id("lb-info-body").flex_1().min_h_0().overflow_y_scroll().p_4().child(content))
            .into_any_element()
    }

    /// Keyboard shortcuts, on "?".
    fn render_lb_keys(&self, cx: &mut Context<Self>) -> AnyElement {
        let keys = [
            ("Left / Right", "browse"),
            ("Z", "zoom explorer"),
            ("M", "on your desks"),
            ("Shift", "hold for the loupe"),
            ("Space", "hold to peek on your real desktop"),
            ("I", "toggle the panel"),
            ("?", "this list"),
            ("Esc", "close"),
        ];
        div()
            .id("lb-keys-overlay")
            .occlude()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(hsla(0., 0., 0., 0.55))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.lb_keys = false;
                    cx.notify();
                }),
            )
            .child(
                v_flex()
                    .gap_2()
                    .p_6()
                    .w(px(380.))
                    .rounded_xl()
                    .bg(hsla(0., 0., 0.06, 0.92))
                    .border_1()
                    .border_color(hsla(0., 0., 1., 0.1))
                    .shadow_lg()
                    .child(div().mb_2().font_weight(FontWeight::SEMIBOLD).text_color(white()).child("Shortcuts"))
                    .children(keys.into_iter().map(|(k, what)| {
                        h_flex()
                            .gap_3()
                            .text_sm()
                            .child(
                                div()
                                    .min_w(px(96.))
                                    .px_2()
                                    .py_0p5()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(hsla(0., 0., 1., 0.18))
                                    .text_xs()
                                    .text_color(hsla(0., 0., 1., 0.85))
                                    .flex()
                                    .justify_center()
                                    .child(k),
                            )
                            .child(div().text_color(hsla(0., 0., 1., 0.65)).child(what))
                    })),
            )
            .into_any_element()
    }
}
