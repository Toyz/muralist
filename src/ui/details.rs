//! The Details card's richer parts: Color X-ray, Fit check, tags grouped by
//! kind, and a strip of similar wallpapers.

use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{Item, Muralist, TAG_LIMIT, icon, pal, purity_color, section, with_alpha};
use crate::thumbs::Job;
use crate::wallhaven::{SearchParams, Sorting, Tag, Wallpaper};

pub(super) enum Similar {
    Loading,
    Ready(Vec<std::sync::Arc<Wallpaper>>),
    Failed,
}

fn rgb_hsla(c: [f32; 3]) -> Hsla {
    Rgba { r: c[0], g: c[1], b: c[2], a: 1. }.into()
}

/// Lookalikes without `like:`: the wallpaper's most telling tag together
/// with its main color, topped up from the tag alone, then the color alone.
fn lookalikes_by_hand(
    client: &crate::wallhaven::Client,
    id: &str,
    purity: [bool; 3],
    categories: [bool; 3],
) -> anyhow::Result<(Vec<Wallpaper>, Option<String>)> {
    let (w, tags) = client.wallpaper(id)?;
    let tag = tags.iter().min_by_key(|t| tag_rank(&t.category).0).map(|t| format!("id:{}", t.id));
    let color = w.colors.first().map(|c| c.trim_start_matches('#').to_owned());
    let search = |query: Option<&String>, color: Option<&String>| {
        client.search(&SearchParams {
            query: query.cloned().unwrap_or_default(),
            categories,
            purity,
            sorting: Sorting::Favorites,
            top_range: "1M".into(),
            ratios: None,
            atleast: None,
            resolutions: None,
            color: color.cloned(),
            page: 1,
            seed: None,
        })
    };
    let mut found: Vec<Wallpaper> = Vec::new();
    let attempts = [(tag.as_ref(), color.as_ref()), (tag.as_ref(), None), (None, color.as_ref())];
    for (q, c) in attempts {
        if found.len() >= 7 || (q.is_none() && c.is_none()) {
            continue;
        }
        if q.is_none() && tag.is_some() && !found.is_empty() {
            // A tag match beats a bare color match.
            break;
        }
        for hit in search(q, c)?.data {
            if hit.id != w.id && !found.iter().any(|f| f.id == hit.id) {
                found.push(hit);
            }
        }
    }
    Ok((found, tag))
}

/// Tag categories by how much they say about a wallpaper.
fn tag_rank(category: &str) -> (u8, &'static str) {
    match category {
        "Characters" => (0, "user"),
        "Anime & Manga" | "Series" | "Games" | "Movies" | "TV Shows" | "Comics" | "Cartoons" => (1, "book-open"),
        "Artists" | "Artist" => (2, "palette"),
        _ => (3, "asterisk"),
    }
}

impl Muralist {
    // ---- Color X-ray -------------------------------------------------------

    /// The selection's image key and the sharpest small copy on hand.
    fn xray_source(&mut self) -> Option<(String, PathBuf)> {
        let item = self.selected.clone()?;
        let (key, full, thumb) = match &item {
            Item::Remote(w) => (w.id.clone(), self.original_on_disk(w), self.remote_thumb(w, true)),
            Item::Local(path) => (super::local_key(path), Some(path.clone()), self.local_thumb(path)),
        };
        // Same file the preview shows, so the mask lines up exactly.
        let fit = full.as_deref().and_then(|f| {
            let w = self.fit_width();
            let h = (self.preview_h() * self.scale).round() as u32;
            self.fitted(f, w, h)
        });
        Some((key, fit.or(thumb)?))
    }

    fn xray_palette(&mut self) -> Option<(String, Vec<([f32; 3], f32)>)> {
        let item = self.selected.clone()?;
        let (key, thumb) = match &item {
            Item::Remote(w) => (w.id.clone(), self.remote_thumb(w, true)?),
            Item::Local(path) => (super::local_key(path), self.local_thumb(path)?),
        };
        if !self.xray_palettes.contains_key(&key) {
            let palette = super::adaptive::weighted_palette(&thumb);
            self.xray_palettes.insert(key.clone(), palette);
        }
        Some((key.clone(), self.xray_palettes.get(&key)?.clone()))
    }

    /// The darkened "only this color" version of the preview, while a
    /// palette band is hovered.
    pub(super) fn xray_overlay(&mut self) -> Option<PathBuf> {
        let index = self.xray?;
        let (_, palette) = self.xray_palette()?;
        let (key, src) = self.xray_source()?;
        let centers: Vec<[f32; 3]> = palette.iter().map(|(c, _)| *c).collect();
        let job_key = format!("xray-{key}-{index}-{:x}", crate::thumbs::path_hash(&src));
        self.thumb(job_key, |k| Job::XRay { key: k, path: src, centers, index })
    }

    pub(super) fn render_xray_palette(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = pal();
        let (_, palette) = self.xray_palette()?;
        if palette.is_empty() {
            return None;
        }
        let hovered = self.xray;
        let n = palette.len();
        let bands: Vec<AnyElement> = palette
            .iter()
            .enumerate()
            .map(|(i, (rgb, share))| {
                let color = rgb_hsla(*rgb);
                let active = hovered == Some(i);
                div()
                    .id(("xray-band", i))
                    .h_full()
                    .w(px((share * self.details_w).max(10.)))
                    .bg(color)
                    .when(i == 0, gpui_kit::Styled::rounded_l_lg)
                    .when(i + 1 == n, gpui_kit::Styled::rounded_r_lg)
                    .cursor_pointer()
                    .when(active, |d| d.border_2().border_color(super::on_accent(color)))
                    .on_hover(cx.listener(move |this, h: &bool, _, cx| {
                        if *h {
                            this.xray = Some(i);
                        } else if this.xray == Some(i) {
                            this.xray = None;
                        }
                        cx.notify();
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.search_by_colors(&[color], "this wallpaper's palette", cx);
                    }))
                    .into_any_element()
            })
            .collect();

        let caption = match hovered.and_then(|i| palette.get(i)) {
            Some((rgb, share)) => {
                let c: Rgba = rgb_hsla(*rgb).into();
                let hex = format!(
                    "#{:02x}{:02x}{:02x}",
                    (c.r * 255.).round() as u8,
                    (c.g * 255.).round() as u8,
                    (c.b * 255.).round() as u8
                );
                format!("{:.0}% of the image  ·  {hex}  ·  click to find this color", share * 100.)
            }
            None => "Hover a band to X-ray where that color lives".into(),
        };

        Some(
            section("Palette", p)
                .child(h_flex().h(px(30.)).w(px(self.details_w)).overflow_hidden().rounded_lg().children(bands))
                .child(div().text_xs().text_color(if hovered.is_some() { p.muted } else { p.dim }).child(caption))
                .into_any_element(),
        )
    }

    // ---- Fit check ------------------------------------------------------------

    // ---- Tags by kind -----------------------------------------------------------

    pub(super) fn render_tag_groups(&mut self, tags: &[Tag], cx: &mut Context<Self>) -> AnyElement {
        let p = pal();
        let mut groups: Vec<(u8, String, &'static str, Vec<&Tag>)> = Vec::new();
        for tag in tags {
            let category = if tag.category.is_empty() { "Other".to_owned() } else { tag.category.clone() };
            let (rank, ico) = tag_rank(&category);
            match groups.iter_mut().find(|g| g.1 == category) {
                Some(g) => g.3.push(tag),
                None => groups.push((rank, category, ico, vec![tag])),
            }
        }
        groups.sort_by(|a, b| a.0.cmp(&b.0).then(b.3.len().cmp(&a.3.len())));

        let total = tags.len();
        let mut budget = if self.tags_expanded { usize::MAX } else { TAG_LIMIT };
        let mut rows: Vec<AnyElement> = Vec::new();
        let mut ix = 0;
        for (_, category, ico, list) in groups {
            if budget == 0 {
                break;
            }
            let shown: Vec<&Tag> = list.into_iter().take(budget).collect();
            budget -= shown.len();
            let chips: Vec<AnyElement> = shown
                .into_iter()
                .map(|tag| {
                    ix += 1;
                    let tint = (tag.purity != crate::wallhaven::Purity::Sfw).then(|| purity_color(tag.purity));
                    let picked = (*tag).clone();
                    h_flex()
                        .id(("tag", ix))
                        .gap_1p5()
                        .px_2p5()
                        .h(px(26.))
                        .rounded_md()
                        .bg(p.bg)
                        .border_1()
                        .border_color(tint.map_or(p.border, |c| with_alpha(c, 0.4)))
                        .text_xs()
                        .text_color(tint.unwrap_or(p.muted))
                        .cursor_pointer()
                        .hover(move |s| s.bg(p.elevated).text_color(p.fg).border_color(p.border_strong))
                        .when_some(tint, |d, c| d.child(div().size(px(5.)).rounded_full().bg(c)))
                        .child(tag.name.clone())
                        .on_click(cx.listener(move |this, _, window, cx| this.search_tag(picked.clone(), window, cx)))
                        // Dropping in one after another.
                        .relative()
                        .with_animation(
                            SharedString::from(format!("tag-in-{}", tag.id)),
                            Animation::new(Duration::from_millis(260 + ix as u64 * 28)),
                            move |d, t| {
                                let delay = ix as f32 * 28.;
                                let t = ((t * (260. + delay) - delay) / 260.).clamp(0., 1.);
                                let e = super::glass::ease_out_cubic(t);
                                d.opacity(e).top(px(-8. * (1. - e)))
                            },
                        )
                        .into_any_element()
                })
                .collect();
            rows.push(
                v_flex()
                    .gap_1()
                    .child(h_flex().gap_1p5().text_xs().text_color(p.dim).child(icon(ico).size_3()).child(category))
                    .child(h_flex().flex_wrap().gap_1p5().children(chips))
                    .into_any_element(),
            );
        }
        let hidden = total.saturating_sub(ix);
        v_flex()
            .gap_2p5()
            .children(rows)
            .when(total > TAG_LIMIT, |d| {
                d.child(
                    div()
                        .id("tags-more")
                        .text_xs()
                        .text_color(p.accent)
                        .cursor_pointer()
                        .child(if hidden > 0 { format!("Show {hidden} more tags") } else { "Show fewer".into() })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.tags_expanded = !this.tags_expanded;
                            cx.notify();
                        })),
                )
            })
            .into_any_element()
    }

    // ---- Similar ------------------------------------------------------------------

    /// Fetch look-alikes once the selection has settled for a moment.
    /// Lookalikes honour the content filters the grid searches with (the
    /// sidebar's plus any the smart query adds), and are fetched again when
    /// those change.
    pub(super) fn load_similar(&mut self, id: String, cx: &mut Context<Self>) {
        let filters = self.current_search(cx).2;
        let mut purity = filters.purity;
        if !self.client.has_key() {
            purity[2] = false;
        }
        let wanted = (purity, filters.categories);
        if self.similar.contains_key(&id) && self.similar_for.get(&id) == Some(&wanted) {
            return;
        }
        self.similar_for.insert(id.clone(), wanted);
        self.similar.insert(id.clone(), Similar::Loading);
        let client = self.client.clone();
        let categories = filters.categories;
        let params = SearchParams {
            query: format!("like:{id}"),
            categories,
            purity,
            sorting: Sorting::Relevance,
            top_range: "1M".into(),
            ratios: None,
            atleast: None,
            resolutions: None,
            color: None,
            page: 1,
            seed: None,
        };
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(700)).await;
            let wanted = this
                .update(cx, |this, _| {
                    let wanted = this.is_selected_id(&id);
                    if !wanted {
                        this.similar.remove(&id);
                    }
                    wanted
                })
                .unwrap_or(false);
            if !wanted {
                return;
            }
            // Wallhaven's front door now challenges `like:` searches as bots,
            // key or not. Once it has, stop asking this session and work out
            // the lookalikes from what still answers: its most telling tag,
            // and its main color.
            let skip_like = this.update(cx, |this, _| this.like_blocked).unwrap_or(false);
            let result = cx
                .background_executor()
                .spawn(async move {
                    let like = if skip_like { None } else { Some(client.search(&params)) };
                    match like {
                        Some(Ok(resp)) => (Ok((resp.data, Some(params.query.clone()))), false),
                        Some(Err(e)) if !format!("{e:#}").contains("403") => (Err(e), false),
                        _ => (lookalikes_by_hand(&client, &params.query[5..], params.purity, params.categories), true),
                    }
                })
                .await;
            this.update(cx, |this, cx| {
                let (result, blocked) = result;
                this.like_blocked |= blocked;
                let state = match result {
                    Ok((list, query)) => {
                        if let Some(q) = query {
                            this.similar_query.insert(id.clone(), q);
                        }
                        // And checked here too, whatever the search let through.
                        let allowed = |w: &Wallpaper| purity[w.purity.index()] && categories[w.category.index()];
                        Similar::Ready(
                            list.into_iter()
                                .filter(|w| w.id != id && allowed(w))
                                .take(6)
                                .map(std::sync::Arc::new)
                                .collect(),
                        )
                    }
                    Err(e) => {
                        eprintln!("similar to {id}: {e:#}");
                        Similar::Failed
                    }
                };
                this.similar.insert(id, state);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

/// "3 days ago" from Wallhaven's "YYYY-MM-DD HH:MM:SS" (UTC).
pub(super) fn ago(created_at: &str) -> Option<(String, bool)> {
    let at = chrono::NaiveDateTime::parse_from_str(created_at, "%Y-%m-%d %H:%M:%S").ok()?.and_utc();
    let secs = (chrono::Utc::now() - at).num_seconds().max(0);
    let fresh = secs < 48 * 3600;
    let (n, unit) = match secs {
        0..60 => return Some(("just now".into(), fresh)),
        60..3600 => (secs / 60, "min"),
        3600..86_400 => (secs / 3600, "h"),
        86_400..1_209_600 => (secs / 86_400, "day"),
        1_209_600..5_184_000 => (secs / 604_800, "week"),
        5_184_000..31_536_000 => (secs / 2_592_000, "month"),
        _ => (secs / 31_536_000, "year"),
    };
    let plural = if n == 1 || unit == "min" || unit == "h" { "" } else { "s" };
    Some((format!("{n} {unit}{plural} ago"), fresh))
}
