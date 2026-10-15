//! Info and Similar for the selected wallpaper, as a case file that
//! unfolds out of the dossier tile you clicked and folds back into it.
//! The picture sits on its own blurred light with focus brackets, its id
//! decoding beneath it over a strip of its palette. The pane's two tabs
//! are headlines with an underline that slides between them. Similar is a
//! lineup ranked by how closely each palette matches; hover one and the
//! picture splits down the middle to put them side by side.

use std::time::Duration;

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    InspectorTab, Item, Muralist, boxed, compact_count, details, develop, file_name, glass, headers, icon, kit, pal,
    parse_hex, purity_color, purity_label, radius, with_alpha,
};

const SHEET_W: f32 = 1100.;
const SHEET_H: f32 = 700.;
/// The details pane on the right.
const PANE_W: f32 = 430.;
const PANE_PAD: f32 = 22.;
const HEAD_H: f32 = 76.;
const OPEN_MS: u64 = 280;
const CLOSE_MS: u64 = 210;
/// Room under the picture for its id and palette.
const HUD_H: f32 = 92.;
const PIC_PAD: f32 = 34.;
const ROW_THUMB: (f32, f32) = (124., 70.);

/// The modal's open or close, growing from (or into) a dossier tile.
pub(super) struct SheetAnim {
    origin: Option<Bounds<Pixels>>,
    seq: u64,
    closing: bool,
}

/// How alike two palettes are, 0 to 1: each color of `a` against its
/// nearest in `b`, hue weighted by how saturated the pair is.
pub(super) fn palette_match(a: &[Hsla], b: &[Hsla]) -> f32 {
    if a.is_empty() || b.is_empty() {
        return 0.5;
    }
    let dist = |x: &Hsla, y: &Hsla| {
        let dh = (x.h - y.h).abs();
        let dh = dh.min(1. - dh) * 2.;
        let sat = (x.s + y.s) / 2.;
        ((dh * sat).powi(2) + (x.s - y.s).powi(2) * 0.5 + (x.l - y.l).powi(2)).sqrt()
    };
    let avg = a.iter().map(|x| b.iter().map(|y| dist(x, y)).fold(f32::MAX, f32::min)).sum::<f32>() / a.len() as f32;
    (1. - avg * 1.6).clamp(0., 1.)
}

fn hexes(colors: &[String]) -> Vec<Hsla> {
    colors.iter().filter_map(|c| parse_hex(c)).collect()
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

impl Muralist {
    pub(super) fn open_sheet(&mut self, tab: InspectorTab, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.is_none() {
            return;
        }
        // Already open: just switch tabs.
        if self.sheet_open() && !self.sheet_anim.as_ref().is_some_and(|a| a.closing) {
            self.inspector_tab = tab;
            cx.notify();
            return;
        }
        let ix = if tab == InspectorTab::Similar { 1 } else { 0 };
        let origin = self.dossier_bounds.borrow().get(ix).copied();
        self.sheet_seq += 1;
        self.sheet_anim = Some(SheetAnim { origin, seq: self.sheet_seq, closing: false });
        self.sheet_compare = None;
        self.inspector_tab = tab;
        self.focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn close_sheet(&mut self, cx: &mut Context<Self>) {
        let Some(anim) = self.sheet_anim.as_mut() else {
            self.inspector_tab = InspectorTab::Set;
            cx.notify();
            return;
        };
        if anim.closing {
            return;
        }
        // Fold back into the tile it came from, then go.
        let ix = if self.inspector_tab == InspectorTab::Similar { 1 } else { 0 };
        anim.origin = self.dossier_bounds.borrow().get(ix).copied().or(anim.origin);
        anim.closing = true;
        self.sheet_seq += 1;
        anim.seq = self.sheet_seq;
        let seq = self.sheet_seq;
        self.sheet_compare = None;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(CLOSE_MS)).await;
            this.update(cx, |this, cx| {
                if this.sheet_anim.as_ref().is_some_and(|a| a.seq == seq) {
                    this.sheet_anim = None;
                    this.inspector_tab = InspectorTab::Set;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
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
        let selected = self.selected.clone()?;
        let tab = self.inspector_tab;

        // A saved file whose origin is known reads as that Wallhaven
        // wallpaper: purity, tags, similar and all.
        let item = match &selected {
            Item::Local(path) => {
                self.provenance.of(path).map(|r| Item::Remote(r.wallpaper.clone())).unwrap_or(selected.clone())
            }
            Item::Remote(_) => selected.clone(),
        };
        let remote = match &item {
            Item::Remote(w) => Some(w.clone()),
            Item::Local(_) => None,
        };
        // The picture: the full original once it's here, the large thumbnail
        // until then; a saved file shows itself.
        let picture = match &selected {
            Item::Remote(w) => self.remote_full(w, cx).or_else(|| self.remote_thumb(w, true)),
            Item::Local(path) => Some(path.clone()),
        };
        let colors = self.selected_colors();
        let c = glass::glow_color(&colors).unwrap_or(p.accent);
        let key = match &item {
            Item::Remote(w) => w.id.clone(),
            Item::Local(path) => file_name(path),
        };

        // ---- geometry ----
        let (vw, vh) = self.viewport;
        let (fw, fh) = (SHEET_W.min(vw * 0.94), SHEET_H.min(vh * 0.9));
        let (fx, fy) = ((vw - fw) / 2., (vh - fh) / 2.);
        let (ox, oy, ow, oh) = match self.sheet_anim.as_ref().and_then(|a| a.origin) {
            Some(b) => {
                (f32::from(b.origin.x), f32::from(b.origin.y), f32::from(b.size.width), f32::from(b.size.height))
            }
            None => (fx + fw * 0.03, fy + fh * 0.03, fw * 0.94, fh * 0.94),
        };
        let (seq, closing) = self.sheet_anim.as_ref().map_or((0, false), |a| (a.seq, a.closing));

        // ---- the picture, on its own light ----
        let (aw, ah) = (fw - PANE_W, fh);
        let aspect = self.selected_dims.map_or(16. / 9., |(w, h)| w as f32 / h.max(1) as f32);
        let (box_w, box_h) = (aw - PIC_PAD * 2., ah - PIC_PAD - HUD_H);
        let pw = box_w.min(box_h * aspect);
        let ph = pw / aspect;
        let (px_, py_) = (PIC_PAD + (box_w - pw) / 2., PIC_PAD + (box_h - ph) / 2.);

        let similar: Vec<crate::wallhaven::Wallpaper> = match remote.as_ref().and_then(|w| self.similar.get(&w.id)) {
            Some(details::Similar::Ready(list)) => list.clone(),
            _ => Vec::new(),
        };
        let mut ranked: Vec<(f32, crate::wallhaven::Wallpaper)> =
            similar.iter().map(|w| (palette_match(&colors, &hexes(&w.colors)), w.clone())).collect();
        ranked.sort_by(|a, b| b.0.total_cmp(&a.0));
        let compare = self.sheet_compare.and_then(|i| ranked.get(i).cloned());
        // Hovering one pulls its original, fitted to cover its half.
        let compare = compare.map(|(m, w)| {
            let thumb = self.remote_thumb(&w, true).or_else(|| self.remote_thumb(&w, false));
            let full = self.remote_full(&w, cx);
            let pulling = match self.full_phase(&Item::Remote(w.clone())) {
                super::FullPhase::Loading(b) => Some(b.fraction.unwrap_or(0.)),
                _ => None,
            };
            let (iw, ih) = w
                .resolution
                .split_once('x')
                .and_then(|(a, b)| Some((a.parse::<f32>().ok()?, b.parse::<f32>().ok()?)))
                .unwrap_or((pw, ph));
            let cover = (pw / 2. / iw).max(ph / ih) * self.scale;
            let fit = full.and_then(|f| self.fitted(&f, (iw * cover).ceil() as u32, (ih * cover).ceil() as u32));
            (m, w.id.clone(), thumb, fit, pulling)
        });
        // The picture itself, downscaled for exactly its box once the
        // original is here, over its thumbnail until then.
        let under = match &selected {
            Item::Remote(w) => self.remote_thumb(w, true).or_else(|| self.remote_thumb(w, false)),
            Item::Local(path) => self.local_thumb(path),
        };
        let sharp = picture
            .as_deref()
            .and_then(|f| self.fitted(f, (pw * self.scale).round() as u32, (ph * self.scale).round() as u32));

        let backdrop = self.backdrop();
        let xray = self.xray_overlay();
        let (dw, dh) = self.selected_dims.unwrap_or((0, 0));
        let guide = hsla(0., 0., 1., 0.09);
        let picture_el = div()
            .absolute()
            .left(px(px_))
            .top(px(py_))
            .w(px(pw))
            .h(px(ph))
            .rounded_lg()
            .overflow_hidden()
            .bg(hsla(0., 0., 0., 0.5))
            .shadow(glass::ambilight(c, 70., 0.45))
            .when_some(under, |d, t| d.child(radius(boxed(img(t), pw, ph).object_fit(ObjectFit::Cover), 8.)))
            .when_some(sharp, |d, t| {
                d.child(super::fade_in(
                    SharedString::from(format!("sheet-sharp-{key}")),
                    radius(boxed(img(t), pw, ph).object_fit(ObjectFit::Cover), 8.),
                ))
            })
            // Hovering a palette color X-rays where it lives.
            .when_some(xray, |d, m| {
                d.child(div().absolute().top_0().left_0().child(boxed(img(m), pw, ph).object_fit(ObjectFit::Cover)))
            })
            // Thirds, and a slow survey beam.
            .children((1..3).flat_map(|i| {
                let at = relative(i as f32 / 3.);
                [
                    div().absolute().top_0().bottom_0().left(at).w(px(1.)).bg(guide).into_any_element(),
                    div().absolute().left_0().right_0().top(at).h(px(1.)).bg(guide).into_any_element(),
                ]
            }))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .h(px(60.))
                    .bg(linear_gradient(
                        180.,
                        linear_color_stop(with_alpha(c, 0.), 0.),
                        linear_color_stop(with_alpha(c, 0.1), 1.),
                    ))
                    .child(div().absolute().left_0().right_0().bottom_0().h(px(1.)).bg(with_alpha(c, 0.35)))
                    .with_animation(
                        "sheet-survey",
                        Animation::new(Duration::from_millis(4800)).repeat().with_easing(ease_in_out),
                        move |d, t| d.top(px(-60. + t * (ph + 60.))),
                    ),
            )
            // Side by side with a hovered lookalike, split down the middle.
            .when_some(compare.clone(), |d, (m, cid, thumb, fit, pulling)| {
                let n = self.sheet_compare.unwrap_or(0);
                let half = |t: std::path::PathBuf, gray: bool| {
                    div()
                        .absolute()
                        .top_0()
                        .right_0()
                        .child(boxed(img(t), pw / 2., ph).object_fit(ObjectFit::Cover).grayscale(gray))
                };
                d.child(
                    div()
                        .absolute()
                        .top_0()
                        .right_0()
                        .h_full()
                        .overflow_hidden()
                        .when_some(thumb, |d, t| d.child(half(t, pulling.is_some())))
                        .when_some(fit, |d, t| {
                            d.child(super::fade_in(SharedString::from(format!("compare-sharp-{cid}")), half(t, false)))
                        })
                        // Still on the wire: say how far along.
                        .when_some(pulling, |d, f| {
                            d.child(
                                h_flex()
                                    .absolute()
                                    .top_3()
                                    .right_3()
                                    .gap_1p5()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .bg(hsla(0., 0., 0., 0.65))
                                    .border_1()
                                    .border_color(with_alpha(c, 0.5))
                                    .text_size(px(9.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(white())
                                    .child(
                                        div().size(px(5.)).rounded_full().bg(c).with_animation(
                                            "compare-rec",
                                            Animation::new(Duration::from_millis(700))
                                                .repeat()
                                                .with_easing(pulsating_between(0.2, 1.)),
                                            |d, t| d.opacity(t),
                                        ),
                                    )
                                    .child(format!("PULLING {}%", (f * 100.) as u32)),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .left_0()
                                    .right_0()
                                    .bottom_0()
                                    .h(px(2.))
                                    .bg(hsla(0., 0., 1., 0.1))
                                    .child(div().h_full().w(relative(f.max(0.02))).bg(c)),
                            )
                        })
                        .child(div().absolute().top_0().bottom_0().left_0().w(px(2.)).bg(c))
                        .with_animation(
                            SharedString::from(format!("compare-{n}")),
                            Animation::new(Duration::from_millis(320)).with_easing(ease_out_quint()),
                            move |d, t| d.w(px(pw / 2. * t)),
                        ),
                )
                .child(
                    h_flex()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .p_3()
                        .justify_between()
                        .text_size(px(10.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(white())
                        .child(div().px_2().py_0p5().rounded_md().bg(hsla(0., 0., 0., 0.6)).child("THIS"))
                        .child(
                            div()
                                .px_2()
                                .py_0p5()
                                .rounded_md()
                                .bg(with_alpha(c, 0.75))
                                .child(format!("LOOKALIKE  {}%", (m * 100.).round() as u32)),
                        ),
                )
            });
        // Focus brackets just outside the picture.
        let brackets = div()
            .absolute()
            .left(px(px_ - 12.))
            .top(px(py_ - 12.))
            .w(px(pw + 24.))
            .h(px(ph + 24.))
            .children((0..4).map(|i| develop::bracket(i, 0., with_alpha(c, 0.85))))
            .when(dw > 0, |d| {
                let coord = |text: String| {
                    div()
                        .absolute()
                        .text_size(px(9.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(with_alpha(c, 0.8))
                        .child(text)
                };
                let g = gcd(dw, dh).max(1);
                d.child(coord("0,0".into()).top(px(-15.)).left_0())
                    .child(coord(format!("{dw},0")).top(px(-15.)).right_0())
                    .child(coord(format!("0,{dh}")).bottom(px(-15.)).left_0())
                    .child(coord(format!("{dw},{dh}")).bottom(px(-15.)).right_0())
                    .child(
                        h_flex().absolute().top(px(-17.)).left_0().right_0().justify_center().child(
                            div()
                                .px_1p5()
                                .rounded_sm()
                                .bg(with_alpha(c, 0.2))
                                .text_size(px(9.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(c)
                                .child(format!("{}:{}  ·  {:.1} MP", dw / g, dh / g, (dw * dh) as f32 / 1e6)),
                        ),
                    )
            });

        // Under the picture: the id decoding, the facts, the palette.
        let decoding = key.clone();
        let facts: Vec<(String, &'static str)> = match &remote {
            Some(w) => {
                let mut f = vec![(w.resolution.clone(), "maximize")];
                f.push((format!("{:.1} MB", w.file_size as f64 / 1_048_576.), "hard-drive"));
                f.push((compact_count(w.favorites), "heart"));
                f.push((w.category.clone(), "tag"));
                f
            }
            None => vec![("saved on this machine".into(), "folder")],
        };
        let hud = v_flex()
            .absolute()
            .left(px(PIC_PAD))
            .right(px(PIC_PAD))
            .bottom(px(22.))
            .gap_2p5()
            .child(
                h_flex()
                    .gap_3()
                    .child(div().text_size(px(22.)).font_weight(FontWeight::BOLD).text_color(white()).with_animation(
                        SharedString::from(format!("sheet-decode-{key}-{seq}")),
                        Animation::new(Duration::from_millis(700)),
                        move |d, t| d.child(headers::scrambled(&decoding, t)),
                    ))
                    .children(remote.as_ref().map(|w| {
                        let pc = purity_color(&w.purity);
                        h_flex()
                            .gap_1()
                            .px_2()
                            .py_0p5()
                            .rounded_full()
                            .bg(with_alpha(pc, 0.2))
                            .text_xs()
                            .text_color(pc)
                            .child(div().size(px(6.)).rounded_full().bg(pc))
                            .child(purity_label(&w.purity))
                    }))
                    .child(div().flex_1())
                    .children(facts.into_iter().map(|(v, i)| {
                        h_flex()
                            .gap_1()
                            .text_xs()
                            .text_color(hsla(0., 0., 1., 0.75))
                            .child(icon(i).size_3().text_color(with_alpha(c, 0.9)))
                            .child(v)
                    })),
            )
            .child(h_flex().h(px(6.)).gap(px(2.)).children(colors.iter().enumerate().map(|(i, col)| {
                div().flex_1().h_full().rounded_full().bg(*col).with_animation(
                    SharedString::from(format!("sheet-pal-{key}-{seq}-{i}")),
                    Animation::new(Duration::from_millis(260 + i as u64 * 60)).with_easing(ease_out_quint()),
                    move |d, t| d.opacity(t),
                )
            })));

        let left = div()
            .absolute()
            .top_0()
            .left_0()
            .w(px(aw))
            .h(px(ah))
            .overflow_hidden()
            .bg(hex_black())
            .when_some(backdrop.map(|(b, _)| b), |d, b| {
                d.child(img(b).absolute().top_0().left_0().size_full().object_fit(ObjectFit::Cover).opacity(0.55))
            })
            .child(div().absolute().top_0().left_0().size_full().bg(hsla(0., 0., 0., 0.45)))
            .child(brackets)
            .child(picture_el)
            .child(hud);

        // ---- the pane ----
        let info_head = match remote.as_ref().and_then(|w| self.info.get(&w.id)) {
            Some(super::InfoState::Ready(i)) => format!("{} tags", i.tags.len()),
            Some(super::InfoState::Failed(_)) => "offline".into(),
            _ if remote.is_none() => "this file".into(),
            _ => "decoding".into(),
        };
        let sim_head = match remote.as_ref().and_then(|w| self.similar.get(&w.id)) {
            Some(details::Similar::Ready(l)) if l.is_empty() => "one of a kind".into(),
            Some(details::Similar::Ready(l)) => format!("{} lookalikes", l.len()),
            Some(details::Similar::Failed) => "offline".into(),
            _ => "searching".into(),
        };
        let tabs_w = PANE_W - 56.;
        let tab_head =
            |id: &'static str, icon_name: &'static str, label: &'static str, value: String, t: InspectorTab| {
                let on = tab == t;
                v_flex()
                    .id(id)
                    .flex_1()
                    .h_full()
                    .justify_center()
                    .gap_0p5()
                    .px(px(PANE_PAD))
                    .cursor_pointer()
                    .child(
                        h_flex()
                            .gap_1()
                            .text_size(px(9.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(if on { c } else { p.dim })
                            .child(icon(icon_name).size(px(10.)))
                            .child(label),
                    )
                    .child(
                        div()
                            .text_base()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(if on { p.fg } else { p.muted })
                            .child(value),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.inspector_tab = t;
                        this.sheet_compare = None;
                        cx.notify();
                    }))
            };
        let on_similar = tab == InspectorTab::Similar;
        let header = h_flex()
            .flex_none()
            .h(px(HEAD_H))
            .border_b_1()
            .border_color(p.border)
            .child(
                h_flex()
                    .relative()
                    .w(px(tabs_w))
                    .h_full()
                    .child(tab_head("sheet-info", "info", "INFO", info_head, InspectorTab::Info))
                    .when(remote.is_some(), |d| {
                        d.child(tab_head("sheet-similar", "images", "SIMILAR", sim_head, InspectorTab::Similar))
                    })
                    // The underline slides to whichever tab is open.
                    .when(remote.is_some(), |d| {
                        let half = tabs_w / 2.;
                        d.child(
                            div()
                                .absolute()
                                .bottom(px(-1.))
                                .h(px(2.))
                                .w(px(half))
                                .bg(c)
                                .shadow(glass::ambilight(c, 10., 0.8))
                                .with_animation(
                                    SharedString::from(format!("sheet-ul-{on_similar}")),
                                    Animation::new(Duration::from_millis(300)).with_easing(ease_out_quint()),
                                    move |d, t| {
                                        let (from, to) = if on_similar { (0., half) } else { (half, 0.) };
                                        d.left(px(lerp(from, to, t)))
                                    },
                                ),
                        )
                    }),
            )
            .child(div().flex_1())
            .child(
                div().pr_3().child(
                    kit::Btn::new("sheet-close")
                        .icon("close")
                        .ghost()
                        .sm()
                        .tooltip("Close (Esc)")
                        .on_click(cx.listener(|this, _, _, cx| this.close_sheet(cx))),
                ),
            );

        // The detail views size themselves from `details_w`.
        let saved_w = self.details_w;
        self.details_w = PANE_W - PANE_PAD * 2.;
        let content: AnyElement = match (tab, &item) {
            (InspectorTab::Similar, Item::Remote(w)) => {
                let w = w.clone();
                self.render_lineup(&w, &ranked, c, cx)
            }
            (_, Item::Remote(w)) => {
                let w = w.clone();
                self.render_case_info(&w, c, cx)
            }
            (_, Item::Local(path)) => {
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
        self.details_w = saved_w;

        let pane = v_flex()
            .absolute()
            .top_0()
            .right_0()
            .w(px(PANE_W))
            .h(px(fh))
            .bg(p.surface)
            .border_l_1()
            .border_color(with_alpha(c, 0.35))
            .child(header)
            .child(
                div()
                    .id(SharedString::from(format!("sheet-body-{key}-{}", tab as u8)))
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(glass::settle(
                        SharedString::from(format!("sheet-tab-{key}-{}", tab as u8)),
                        div().p(px(PANE_PAD)).child(content),
                    )),
            );

        let card = div()
            .id("sheet-card")
            .absolute()
            .rounded_xl()
            .overflow_hidden()
            .bg(p.bg)
            .border_1()
            .border_color(with_alpha(c, 0.5))
            .shadow(glass::ambilight(c, 90., 0.3))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(left)
            .child(pane)
            .child(comet("sheet-comet-top", c, fw, false))
            .child(comet("sheet-comet-bottom", c, fw, true))
            .with_animation(
                SharedString::from(format!("sheet-grow-{seq}")),
                Animation::new(Duration::from_millis(if closing { CLOSE_MS } else { OPEN_MS })),
                move |d, t| {
                    // Out along a quint, back in along a cubic.
                    let e = if closing { 1. - t.powi(3) } else { 1. - (1. - t).powi(5) };
                    d.left(px(lerp(ox, fx, e)))
                        .top(px(lerp(oy, fy, e)))
                        .w(px(lerp(ow, fw, e)))
                        .h(px(lerp(oh, fh, e)))
                        .opacity(0.4 + 0.6 * e)
                },
            );

        let shade = div().absolute().top_0().left_0().size_full().bg(hsla(0., 0., 0., 0.65)).with_animation(
            SharedString::from(format!("sheet-shade-{seq}")),
            Animation::new(Duration::from_millis(if closing { CLOSE_MS } else { OPEN_MS })),
            move |d, t| d.opacity(if closing { 1. - t } else { t }),
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
                .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                    if ev.keystroke.key == "escape" {
                        this.close_sheet(cx);
                        cx.stop_propagation();
                    }
                }))
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.close_sheet(cx)))
                .child(shade)
                .child(card)
                .into_any_element(),
        )
    }

    /// The lookalikes, best palette match first.
    fn render_lineup(
        &mut self,
        w: &crate::wallhaven::Wallpaper,
        ranked: &[(f32, crate::wallhaven::Wallpaper)],
        c: Hsla,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = pal();
        let (tw, th) = ROW_THUMB;
        let state = self.similar.get(&w.id);
        let loading = matches!(state, None | Some(details::Similar::Loading));
        let failed = matches!(state, Some(details::Similar::Failed));
        let see_all = self.similar_query.get(&w.id).cloned();
        let ours = self.selected_colors();

        let head = h_flex()
            .justify_between()
            .child(div().text_size(px(9.)).font_weight(FontWeight::SEMIBOLD).text_color(p.dim).child(if loading {
                "SEARCHING BY TAG AND COLOR"
            } else {
                "RANKED BY PALETTE MATCH"
            }))
            .children(see_all.filter(|_| !ranked.is_empty()).map(|q| {
                kit::Btn::new("similar-all").label("See all").icon("arrow-right").ghost().sm().on_click(cx.listener(
                    move |this, _, window, cx| {
                        this.close_sheet(cx);
                        this.search_for(q.clone(), window, cx)
                    },
                ))
            }));

        let rows: Vec<AnyElement> = if loading {
            (0..5)
                .map(|i| {
                    h_flex()
                        .gap_3()
                        .child(
                            div()
                                .relative()
                                .overflow_hidden()
                                .w(px(tw))
                                .h(px(th))
                                .rounded_md()
                                .bg(p.bg)
                                .child(super::shimmer(SharedString::from(format!("lineup-sk-{i}")))),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .gap_2()
                                .child(div().w(px(70.)).h(px(12.)).rounded_sm().bg(p.surface2))
                                .child(div().w_full().h(px(4.)).rounded_full().bg(p.surface2)),
                        )
                        .into_any_element()
                })
                .collect()
        } else {
            ranked
                .iter()
                .enumerate()
                .map(|(i, (m, sw))| {
                    let thumb = self.remote_thumb(sw, false);
                    let pc = purity_color(&sw.purity);
                    let theirs = hexes(&sw.colors);
                    let hovered = self.sheet_compare == Some(i);
                    let item = Item::Remote(sw.clone());
                    let pct = (m * 100.).round() as u32;
                    let m = *m;
                    let row = h_flex()
                        .id(("lineup", i))
                        .gap_3()
                        .p_1p5()
                        .rounded_lg()
                        .border_1()
                        .border_color(if hovered { c } else { gpui_kit::transparent_black() })
                        .bg(if hovered { with_alpha(c, 0.12) } else { gpui_kit::transparent_black() })
                        .cursor_pointer()
                        .child(
                            div()
                                .relative()
                                .flex_none()
                                .w(px(tw))
                                .h(px(th))
                                .rounded_md()
                                .bg(p.bg)
                                .when_some(thumb, |d, t| {
                                    d.child(radius(boxed(img(t), tw, th).object_fit(ObjectFit::Cover), 6.))
                                })
                                .child(
                                    div()
                                        .absolute()
                                        .top_1()
                                        .left_1()
                                        .px_1()
                                        .rounded_sm()
                                        .bg(match i {
                                            0 => hsla(0.12, 0.9, 0.55, 0.95),
                                            1 => hsla(0.6, 0.15, 0.78, 0.95),
                                            2 => hsla(0.07, 0.7, 0.45, 0.95),
                                            _ => hsla(0., 0., 0., 0.65),
                                        })
                                        .text_size(px(9.))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(if i < 3 { hsla(0., 0., 0.08, 1.) } else { white() })
                                        .child(format!("#{}", i + 1)),
                                ),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap_1p5()
                                .child(
                                    h_flex()
                                        .items_end()
                                        .gap_1()
                                        .child(
                                            div()
                                                .text_size(px(20.))
                                                .line_height(px(20.))
                                                .font_weight(FontWeight::BOLD)
                                                .text_color(if hovered { c } else { p.fg })
                                                .child(format!("{pct}")),
                                        )
                                        .child(div().text_xs().text_color(p.dim).child("% match")),
                                )
                                .child(
                                    div().w_full().h(px(4.)).rounded_full().bg(p.surface2).child(
                                        div().h_full().rounded_full().bg(c).with_animation(
                                            SharedString::from(format!("lineup-bar-{}-{i}", sw.id)),
                                            Animation::new(Duration::from_millis(500 + i as u64 * 70))
                                                .with_easing(ease_out_quint()),
                                            move |d, t| d.w(relative(m.max(0.03) * t)),
                                        ),
                                    ),
                                )
                                .child(
                                    h_flex()
                                        .gap_2()
                                        .text_xs()
                                        .text_color(p.dim)
                                        .child(div().size(px(6.)).rounded_full().bg(pc))
                                        .child(sw.resolution.clone())
                                        .child(
                                            h_flex()
                                                .gap_0p5()
                                                .child(icon("heart").size_3())
                                                .child(compact_count(sw.favorites)),
                                        )
                                        .child(div().flex_1())
                                        // This palette over theirs.
                                        .child(
                                            v_flex()
                                                .gap(px(1.))
                                                .child(h_flex().gap(px(1.)).children(
                                                    ours.iter().take(5).map(|c| div().w(px(9.)).h(px(5.)).bg(*c)),
                                                ))
                                                .child(h_flex().gap(px(1.)).children(
                                                    theirs.iter().take(5).map(|c| div().w(px(9.)).h(px(5.)).bg(*c)),
                                                )),
                                        ),
                                ),
                        )
                        .on_hover(cx.listener(move |this, over: &bool, _, cx| {
                            let next = if *over { Some(i) } else { this.sheet_compare.filter(|h| *h != i) };
                            if this.sheet_compare != next {
                                this.sheet_compare = next;
                                cx.notify();
                            }
                        }))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.sheet_compare = None;
                            this.select(item.clone(), window, cx)
                        }));
                    glass::stagger(SharedString::from(format!("lineup-{}-{i}", sw.id)), i, row).into_any_element()
                })
                .collect()
        };
        let empty = (!loading && ranked.is_empty()).then(|| {
            div().text_sm().text_color(p.dim).child(if failed {
                "Couldn't search Wallhaven right now."
            } else {
                "Nothing else quite like it."
            })
        });

        v_flex().gap_3().child(head).children(rows).children(empty).into_any_element()
    }
}

/// A short streak of light running along one edge of the card.
fn comet(id: &'static str, c: Hsla, span: f32, reverse: bool) -> AnimationElement<Div> {
    const LEN: f32 = 160.;
    div()
        .absolute()
        .map(|d| if reverse { d.bottom_0() } else { d.top_0() })
        .h(px(1.5))
        .w(px(LEN))
        .bg(linear_gradient(90., linear_color_stop(with_alpha(c, 0.), 0.), linear_color_stop(with_alpha(c, 1.), 0.5)))
        .with_animation(
            id,
            Animation::new(Duration::from_millis(3400)).repeat().with_easing(ease_in_out),
            move |d, t| {
                let t = if reverse { 1. - t } else { t };
                d.left(px(-LEN + t * (span + LEN)))
            },
        )
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

fn hex_black() -> Hsla {
    hsla(0., 0., 0., 1.)
}

#[cfg(test)]
mod tests {
    use super::palette_match;
    use gpui_kit::hsla;

    #[test]
    fn palettes_match_themselves_best() {
        let warm = [hsla(0.02, 0.8, 0.5, 1.), hsla(0.08, 0.7, 0.6, 1.)];
        let cold = [hsla(0.6, 0.8, 0.5, 1.), hsla(0.55, 0.7, 0.4, 1.)];
        assert!((palette_match(&warm, &warm) - 1.).abs() < 1e-4);
        assert!(palette_match(&warm, &cold) < palette_match(&warm, &warm));
        assert_eq!(palette_match(&[], &cold), 0.5);
    }
}
