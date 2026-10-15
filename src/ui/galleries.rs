//! Galleries in the UI: the Galleries tab, the save-to-gallery picker, the
//! drop dock that rises while a card is dragged, and the background work
//! (refreshing smart and Wallhaven galleries, keeping originals offline).

use gpui_kit::component::input::Input;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    GRID_PAD, Item, Muralist, Pal, Tab, bar_field, drag, icon, keycap, kit, no_drag, pal, side_card_with, with_alpha,
};
use crate::config::RotateSource;
use crate::gallery::{Entry, Gallery, SMART_LIMIT, SavedSearch, Source, fetch_source};
use crate::wallhaven::Collection;

/// Smart and Wallhaven galleries older than this refresh when opened.
const STALE_SECS: i64 = 24 * 3600;
/// Auto-rotate interval used when a gallery turns rotation on.
const DEFAULT_ROTATE_MINUTES: u32 = 30;

/// Background work on one gallery.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum GalleryWork {
    Refreshing,
    Downloading { done: usize, total: usize },
}

pub(super) enum ImportState {
    Loading,
    Ready(Vec<Collection>),
    Failed(String),
}

/// The save-to-gallery popover: for which wallpaper, and where.
pub(super) struct Picker {
    pub item: Item,
    pub at: Point<Pixels>,
}

pub(super) fn entry_of(item: &Item) -> Entry {
    match item {
        Item::Remote(w) => Entry::Wallhaven(w.clone()),
        Item::Local(p) => Entry::Local(p.clone()),
    }
}

pub(super) fn item_of(entry: &Entry) -> Item {
    match entry {
        Entry::Wallhaven(w) => Item::Remote(w.clone()),
        Entry::Local(p) => Item::Local(p.clone()),
    }
}

fn kind_icon(source: &Source) -> &'static str {
    match source {
        Source::Manual => "images",
        Source::Smart { .. } => "sparkles",
        Source::Wallhaven { .. } => "globe",
    }
}

fn describe(source: &Source) -> String {
    match source {
        Source::Manual => "Hand-picked".into(),
        Source::Smart { search, .. } => search.describe(),
        Source::Wallhaven { user, .. } => format!("@{user}'s collection on Wallhaven"),
    }
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn is_image(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "jpg" | "jpeg" | "png" | "webp" | "bmp" | "gif"))
}

impl Muralist {
    pub(super) fn gallery_items(&self) -> Vec<Item> {
        self.gallery
            .and_then(|id| self.galleries.get(id))
            .map(|g| g.items.iter().map(item_of).collect())
            .unwrap_or_default()
    }

    /// Galleries a person adds to by hand (smart and imported ones refill
    /// themselves, so they aren't offered).
    fn manual_galleries(&self) -> Vec<(u64, String, usize)> {
        self.galleries
            .list
            .iter()
            .filter(|g| g.source == Source::Manual)
            .map(|g| (g.id, g.name.clone(), g.items.len()))
            .collect()
    }

    fn save_galleries(&mut self, cx: &mut Context<Self>) {
        if let Err(e) = self.galleries.save() {
            self.set_status(format!("Saving galleries failed: {e:#}"), true, cx);
        }
    }

    pub(super) fn open_gallery(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.gallery != Some(id) || self.tab != Tab::Galleries {
            self.deal();
        }
        self.tab = Tab::Galleries;
        self.gallery = Some(id);
        self.confirm_delete = None;
        let stale = self.galleries.get(id).is_some_and(|g| g.source.refreshable() && now() - g.refreshed > STALE_SECS);
        if stale {
            self.refresh_gallery(id, cx);
        }
        cx.notify();
    }

    /// Opens the Galleries tab on the last gallery shown, or the first.
    pub(super) fn show_galleries(&mut self, cx: &mut Context<Self>) {
        let id =
            self.gallery.filter(|id| self.galleries.get(*id).is_some()).or(self.galleries.list.first().map(|g| g.id));
        if let Some(id) = id {
            self.open_gallery(id, cx)
        } else {
            self.tab = Tab::Galleries;
            self.gallery = None;
            cx.notify();
        }
    }

    pub(super) fn new_gallery(&mut self, first: Option<Item>, cx: &mut Context<Self>) -> u64 {
        let name = self.galleries.unique_name("Gallery");
        let id = self.galleries.create(name, Source::Manual);
        if let Some(item) = first {
            self.galleries.add(id, entry_of(&item));
        }
        self.save_galleries(cx);
        cx.notify();
        id
    }

    /// Adds items dropped or picked into a gallery, and says so.
    pub(super) fn add_to_gallery(&mut self, id: u64, items: Vec<Item>, cx: &mut Context<Self>) {
        let added = items.iter().filter(|item| self.galleries.add(id, entry_of(item))).count();
        let name = self.galleries.get(id).map(|g| g.name.clone()).unwrap_or_default();
        self.save_galleries(cx);
        let msg = match added {
            0 => format!("Already in {name}"),
            1 => format!("Added to {name}"),
            n => format!("Added {n} to {name}"),
        };
        self.set_status(msg, false, cx);
        if self.galleries.get(id).is_some_and(|g| g.offline) {
            self.sync_offline(id, cx);
        }
        cx.notify();
    }

    fn toggle_in_gallery(&mut self, id: u64, item: &Item, cx: &mut Context<Self>) {
        let now_in = self.galleries.toggle(id, entry_of(item));
        self.save_galleries(cx);
        if now_in && self.galleries.get(id).is_some_and(|g| g.offline) {
            self.sync_offline(id, cx);
        }
        cx.notify();
    }

    /// Freezes the current search into a smart gallery.
    pub(super) fn save_search_as_gallery(&mut self, cx: &mut Context<Self>) {
        let mut search = SavedSearch::from_params(&self.current_search(cx).2);
        // Name a scoped search for what it is, not its token.
        search.label = self.scope.clone().and_then(|scope| match scope {
            super::smartq::Scope::Tag(id) => self.tag_names.get(&id).cloned().flatten().map(|t| t.name),
            super::smartq::Scope::User(name) => Some(format!("@{name}'s uploads")),
            super::smartq::Scope::Like(id) => Some(format!("Similar to {id}")),
        });
        let name = self.galleries.unique_name(&search.suggested_name());
        let id = self.galleries.create(name.clone(), Source::Smart { search, limit: SMART_LIMIT });
        // Start with what's on screen; the refresh tops it up.
        if let Some(g) = self.galleries.get_mut(id) {
            g.items = self.results.iter().take(SMART_LIMIT as usize).cloned().map(Entry::Wallhaven).collect();
        }
        self.save_galleries(cx);
        self.refresh_gallery(id, cx);
        self.set_status(format!("Saved as the smart gallery \"{name}\""), false, cx);
    }

    pub(super) fn delete_gallery(&mut self, id: u64, cx: &mut Context<Self>) {
        self.galleries.remove(id);
        self.gallery_work.remove(&id);
        self.confirm_delete = None;
        if self.cfg.rotate_source == RotateSource::Gallery(id) {
            self.cfg.rotate_source = RotateSource::Search;
            self.save_cfg_quietly();
        }
        self.save_galleries(cx);
        self.gallery = self.galleries.list.first().map(|g| g.id);
        self.selected = None;
        cx.notify();
    }

    fn save_cfg_quietly(&self) {
        if let Err(e) = self.cfg.save() {
            eprintln!("saving config: {e:#}");
        }
    }

    pub(super) fn refresh_gallery(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(g) = self.galleries.get(id) else { return };
        if !g.source.refreshable() || self.gallery_work.contains_key(&id) {
            return;
        }
        let source = g.source.clone();
        let client = self.client.clone();
        let mut purity = self.purity;
        if !self.client.has_key() {
            purity[2] = false;
        }
        self.gallery_work.insert(id, GalleryWork::Refreshing);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let items = fetch_source(&client, &source, purity)?;
                    // A search saved before labels existed gets one now.
                    let label = match &source {
                        Source::Smart { search, .. } if search.label.is_none() => search.resolve_label(&client),
                        _ => None,
                    };
                    anyhow::Ok((items, label))
                })
                .await;
            this.update(cx, |this, cx| {
                this.gallery_work.remove(&id);
                match result {
                    Ok((items, label)) => {
                        let Some(g) = this.galleries.get_mut(id) else { return };
                        if let (Some(label), Source::Smart { search, .. }) = (label, &mut g.source) {
                            // Renamed only if it still carries the raw query as its name.
                            if g.name.trim() == search.query.trim() {
                                g.name = label.clone();
                            }
                            search.label = Some(label);
                        }
                        g.items = items;
                        g.refreshed = now();
                        let offline = g.offline;
                        this.deal();
                        this.save_galleries(cx);
                        if offline {
                            this.sync_offline(id, cx);
                        }
                    }
                    Err(e) => this.set_status(format!("Refreshing the gallery failed: {e:#}"), true, cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn set_offline(&mut self, id: u64, on: bool, cx: &mut Context<Self>) {
        if let Some(g) = self.galleries.get_mut(id) {
            g.offline = on;
        }
        self.save_galleries(cx);
        if on {
            self.sync_offline(id, cx);
        }
        cx.notify();
    }

    /// Downloads any originals an offline gallery is missing, one at a time
    /// so it never competes with what you're looking at.
    pub(super) fn sync_offline(&mut self, id: u64, cx: &mut Context<Self>) {
        let dir = self.cfg.download_dir.clone();
        let Some(g) = self.galleries.get(id) else { return };
        if !g.offline || self.gallery_work.contains_key(&id) {
            return;
        }
        let missing: Vec<Item> = g.missing_originals(&dir).map(item_of).collect();
        if missing.is_empty() {
            return;
        }
        let total = missing.len();
        let client = self.client.clone();
        self.gallery_work.insert(id, GalleryWork::Downloading { done: 0, total });
        cx.notify();
        cx.spawn(async move |this, cx| {
            let mut failed = 0;
            for (i, item) in missing.into_iter().enumerate() {
                // Stop if offline was switched off or the gallery deleted.
                let keep =
                    this.update(cx, |this, _| this.galleries.get(id).is_some_and(|g| g.offline)).unwrap_or(false);
                if !keep {
                    break;
                }
                let (client, dir) = (client.clone(), dir.clone());
                let ok = cx.background_executor().spawn(async move { Self::fetch(&client, &dir, item) }).await.is_ok();
                failed += usize::from(!ok);
                this.update(cx, |this, cx| {
                    this.gallery_work.insert(id, GalleryWork::Downloading { done: i + 1, total });
                    cx.notify();
                })
                .ok();
            }
            this.update(cx, |this, cx| {
                this.gallery_work.remove(&id);
                this.refresh_library();
                if failed > 0 {
                    this.set_status(format!("{failed} originals couldn't be downloaded"), true, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Finishes offline downloads interrupted by quitting.
    pub(super) fn resume_galleries(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<u64> = self.galleries.list.iter().filter(|g| g.offline).map(|g| g.id).collect();
        for id in ids {
            self.sync_offline(id, cx);
        }
    }

    fn load_collections(&mut self, cx: &mut Context<Self>) {
        self.import = Some(ImportState::Loading);
        let client = self.client.clone();
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { client.collections() }).await;
            this.update(cx, |this, cx| {
                this.import = Some(match result {
                    Ok(list) => ImportState::Ready(list),
                    Err(e) => ImportState::Failed(format!("{e:#}")),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn import_collection(&mut self, c: &Collection, cx: &mut Context<Self>) {
        let user = self.wh_user.read(cx).value().trim().trim_start_matches('@').to_owned();
        if user.is_empty() {
            self.set_status("Enter your Wallhaven username to import", true, cx);
            return;
        }
        if self.cfg.wallhaven_user != user {
            self.cfg.wallhaven_user = user.clone();
            self.save_cfg_quietly();
        }
        let existing = self.galleries.list.iter().find(
            |g| matches!(&g.source, Source::Wallhaven { user: u, collection, .. } if *u == user && *collection == c.id),
        );
        let id = if let Some(g) = existing {
            g.id
        } else {
            let name = self.galleries.unique_name(&c.label);
            self.galleries.create(name, Source::Wallhaven { user, collection: c.id, purity: None })
        };
        self.save_galleries(cx);
        self.open_gallery(id, cx);
        self.refresh_gallery(id, cx);
    }

    pub(super) fn start_rename(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self.galleries.get(id).map(|g| g.name.clone()) else { return };
        self.renaming = Some(id);
        self.gallery_name.update(cx, |s, cx| s.set_value(name, window, cx));
        self.gallery_name.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    pub(super) fn commit_rename(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.renaming.take() else { return };
        let name = self.gallery_name.read(cx).value().trim().to_owned();
        if let Some(g) = self.galleries.get_mut(id).filter(|_| !name.is_empty()) {
            g.name = name;
        }
        self.save_galleries(cx);
        cx.notify();
    }

    /// Turns auto-rotate on from this gallery, or off if it already is.
    pub(super) fn toggle_rotation(&mut self, id: u64, cx: &mut Context<Self>) {
        let on = self.cfg.rotate_source == RotateSource::Gallery(id) && self.cfg.rotate_minutes > 0;
        if on {
            self.cfg.rotate_minutes = 0;
            self.set_status("Auto-rotate off", false, cx);
        } else {
            self.cfg.rotate_source = RotateSource::Gallery(id);
            self.cfg.rotate_cursor = 0;
            if self.cfg.rotate_minutes == 0 {
                self.cfg.rotate_minutes = DEFAULT_ROTATE_MINUTES;
            }
            let name = self.galleries.get(id).map(|g| g.name.clone()).unwrap_or_default();
            self.set_status(format!("Rotating from {name} every {} min", self.cfg.rotate_minutes), false, cx);
        }
        self.save_cfg_quietly();
        cx.notify();
    }

    fn gallery_covers(&mut self, g_ix: usize, n: usize) -> Vec<std::path::PathBuf> {
        let entries: Vec<Entry> = self.galleries.list[g_ix].items.iter().take(n).cloned().collect();
        entries
            .iter()
            .filter_map(|e| match e {
                Entry::Wallhaven(w) => self.remote_thumb(w, false),
                Entry::Local(p) => self.local_thumb(p),
            })
            .collect()
    }

    // ---- rendering ---------------------------------------------------------------

    /// The Galleries tab's sidebar: the list, plus Wallhaven import.
    pub(super) fn render_gallery_side(&mut self, cx: &mut Context<Self>) -> Div {
        let p = pal();
        let rows: Vec<AnyElement> =
            (0..self.galleries.list.len()).map(|ix| self.render_gallery_row(ix, cx).into_any_element()).collect();
        let count = self.galleries.list.len();
        let has_key = self.client.has_key();

        let list = side_card_with("images", "Galleries", p, |h| {
            h.child(kit::Btn::new("new-gallery").icon("plus").ghost().sm().tooltip("New gallery").on_click(
                cx.listener(|this, _, _, cx| {
                    let id = this.new_gallery(None, cx);
                    this.open_gallery(id, cx);
                }),
            ))
            .when(has_key, |d| {
                d.child(
                    kit::Btn::new("import-wh")
                        .icon("download")
                        .ghost()
                        .sm()
                        .tooltip("Import a Wallhaven collection")
                        .on_click(cx.listener(|this, _, _, cx| this.load_collections(cx))),
                )
            })
        })
        .gap_2()
        .child(v_flex().gap_2().children(rows).on_children_prepainted({
            // Where each tile sits, for the right-click popout to lift from.
            let bounds = self.gallery_tile_bounds.clone();
            let ids: Vec<u64> = self.galleries.list.iter().map(|g| g.id).collect();
            move |rects: Vec<Bounds<Pixels>>, _, _| {
                let mut map = bounds.borrow_mut();
                map.clear();
                map.extend(ids.iter().copied().zip(rects));
            }
        }))
        .when(count == 0, |d| {
            d.child(div().text_xs().text_color(p.muted).child(
                "Collect wallpapers into galleries, then rotate through them. Drag a card onto a gallery to add it.",
            ))
        });

        v_flex().gap_3().child(list).children(self.render_import(cx))
    }

    /// One gallery as a picture tile, like the tab tiles: grayscale at rest,
    /// in color on hover or when open, with its actions on hover.
    fn render_gallery_row(&mut self, ix: usize, cx: &mut Context<Self>) -> Stateful<Div> {
        const R: f32 = 8.;
        const H: f32 = 58.;
        // The card's width inside the sidebar padding, card border and padding.
        let row_w = (self.side_w - 1. - 24. - 2. - super::SIDE_CARD_PAD * 2.).floor();
        let p = pal();
        let covers = self.gallery_covers(ix, 1);
        let g: &Gallery = &self.galleries.list[ix];
        let (id, name, n, offline) = (g.id, g.name.clone(), g.items.len(), g.offline);
        let kind = kind_icon(&g.source);
        let manual = g.source == Source::Manual;
        let colors: Vec<Hsla> = g
            .items
            .iter()
            .take(3)
            .flat_map(|e| super::gridmodes::item_colors(&item_of(e), &self.local_palettes))
            .collect();
        let c = super::glass::glow_color(&colors).unwrap_or(p.accent);
        let open = self.tab == Tab::Galleries && self.gallery == Some(id);
        let rotating = self.cfg.rotate_source == RotateSource::Gallery(id) && self.cfg.rotate_minutes > 0;
        let work = self.gallery_work.get(&id).copied();
        let deleting = self.confirm_delete == Some(id);
        let group = SharedString::from(format!("gallery-{id}"));

        let picture = |gray: bool| {
            covers.first().cloned().map(|t| {
                // Sized to the tile exactly (see `boxed`).
                div().absolute().top(px(1.)).left(px(1.)).w(px(row_w - 2.)).h(px(H - 2.)).child(super::radius(
                    super::boxed(img(t), row_w - 2., H - 2.).object_fit(ObjectFit::Cover).grayscale(gray),
                    R - 1.,
                ))
            })
        };
        let status: Option<AnyElement> = match work {
            Some(GalleryWork::Refreshing) => Some(Spinner::new().xsmall().color(white()).into_any_element()),
            Some(GalleryWork::Downloading { done, total }) => {
                Some(div().child(format!("{done}/{total}")).into_any_element())
            }
            None => None,
        };
        let action = |aid: &'static str, icon_name: &'static str, tip: &'static str, on: bool| {
            div()
                .id(aid)
                .size(px(24.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_md()
                .cursor_pointer()
                .bg(if on { with_alpha(c, 0.45) } else { hsla(0., 0., 0., 0.55) })
                .text_color(white())
                .hover(|s| s.bg(hsla(0., 0., 0., 0.8)))
                .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
                .child(icon(icon_name).size_3p5())
        };
        let actions = h_flex()
            .absolute()
            .top(px(6.))
            .right(px(6.))
            .gap_1()
            // Shown on hover, or while a delete waits for its second click.
            .when(!deleting, |d| d.invisible().group_hover(group.clone(), gpui_kit::Styled::visible))
            .child(action("g-rename", "pencil", "Rename", false).on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.open_gallery(id, cx);
                this.start_rename(id, window, cx);
            })))
            .child(
                action(
                    "g-offline",
                    "hard-drive-download",
                    if offline { "Originals kept offline. Click to stop" } else { "Keep originals offline" },
                    offline,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    let on = this.galleries.get(id).is_some_and(|g| g.offline);
                    this.set_offline(id, !on, cx);
                })),
            )
            .child(if deleting {
                div()
                    .id("g-delete")
                    .h(px(24.))
                    .px_2()
                    .flex()
                    .items_center()
                    .rounded_md()
                    .cursor_pointer()
                    .bg(p.error)
                    .text_xs()
                    .text_color(white())
                    .child("Delete?")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.delete_gallery(id, cx);
                    }))
            } else {
                action("g-delete", "trash", "Delete gallery", false).on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.confirm_delete = Some(id);
                    cx.notify();
                }))
            });

        div()
            .id(("gallery-row", id))
            .group(group.clone())
            .relative()
            .w(px(row_w))
            .h(px(H))
            .map(|d| super::radius(d, R))
            .overflow_hidden()
            .cursor_pointer()
            .bg(linear_gradient(
                135.,
                linear_color_stop(with_alpha(c, if open { 0.35 } else { 0.14 }), 0.),
                linear_color_stop(p.surface2, 1.),
            ))
            .when(covers.is_empty(), |d| {
                d.child(
                    div()
                        .absolute()
                        .left(px(12.))
                        .top(px(10.))
                        .child(icon(kind).size(px(22.)).text_color(with_alpha(c, 0.35))),
                )
            })
            .map(|d| {
                if open {
                    d.children(picture(false))
                } else {
                    d.children(picture(true)).child(
                        div()
                            .absolute()
                            .inset_0()
                            .invisible()
                            .group_hover(group.clone(), gpui_kit::Styled::visible)
                            .children(picture(false)),
                    )
                }
            })
            .child(div().absolute().inset_0().map(|d| super::radius(d, R)).bg(linear_gradient(
                90.,
                linear_color_stop(hsla(0., 0., 0., 0.78), 0.),
                linear_color_stop(hsla(0., 0., 0., if open { 0.05 } else { 0.3 }), 1.),
            )))
            .child(
                v_flex()
                    .absolute()
                    .left(px(12.))
                    .right(px(86.))
                    .top_0()
                    .bottom_0()
                    .justify_center()
                    .child(
                        div()
                            .truncate()
                            .text_sm()
                            .font_weight(if open { FontWeight::SEMIBOLD } else { FontWeight::MEDIUM })
                            .text_color(white())
                            .child(name),
                    )
                    .child(
                        h_flex()
                            .gap_1p5()
                            .text_xs()
                            .text_color(hsla(0., 0., 1., 0.7))
                            .when(!manual, |d| d.child(icon(kind).size_3()))
                            .child(format!("{n} wallpaper{}", if n == 1 { "" } else { "s" }))
                            .when(offline, |d| d.child(icon("hard-drive-download").size_3()))
                            .when(rotating, |d| d.child(icon("shuffle").size_3().text_color(c))),
                    ),
            )
            .child(actions)
            // Busy state as a corner badge, so the tile never reflows.
            .children(status.map(|s| {
                h_flex()
                    .absolute()
                    .bottom(px(6.))
                    .right(px(8.))
                    .h(px(16.))
                    .px_1p5()
                    .gap_1()
                    .rounded_full()
                    .bg(hsla(0., 0., 0., 0.55))
                    .text_size(px(10.))
                    .text_color(white())
                    .child(s)
            }))
            // The edge last, so the picture can't paint over it.
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .map(|d| super::radius(d, R))
                    .border_1()
                    .border_color(if open { with_alpha(c, 0.8) } else { with_alpha(p.border, 0.6) })
                    .when(!open, |d| d.group_hover(group.clone(), move |s| s.border_color(with_alpha(c, 0.5)))),
            )
            // Only hand-picked galleries take drops; the others refill themselves.
            .when(manual, |d| {
                d.drag_over::<drag::CardDrag>(move |s, _, _, _| s.opacity(0.75))
                    .drag_over::<ExternalPaths>(move |s, _, _, _| s.opacity(0.75))
                    .on_drop(cx.listener(move |this, drag: &drag::CardDrag, _, cx| {
                        this.add_to_gallery(id, vec![drag.item.clone()], cx);
                    }))
                    .on_drop(cx.listener(move |this, paths: &ExternalPaths, _, cx| {
                        let items = paths.paths().iter().filter(|p| is_image(p)).cloned().map(Item::Local).collect();
                        this.add_to_gallery(id, items, cx);
                    }))
            })
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                    let origin = this.gallery_tile_bounds.borrow().get(&id).copied();
                    let origin = origin.unwrap_or_else(|| Bounds::new(ev.position, size(px(240.), px(58.))));
                    this.open_gallery_pop(id, origin, cx);
                    cx.stop_propagation();
                }),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.open_gallery(id, cx)))
    }

    fn render_import(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        let p = pal();
        let state = self.import.as_ref()?;
        let body: AnyElement = match state {
            ImportState::Loading => h_flex()
                .gap_2()
                .text_xs()
                .text_color(p.muted)
                .child(Spinner::new().xsmall().color(p.accent))
                .child("Fetching your collections")
                .into_any_element(),
            ImportState::Failed(e) => div().text_xs().text_color(p.error).child(e.clone()).into_any_element(),
            ImportState::Ready(list) if list.is_empty() => div()
                .text_xs()
                .text_color(p.muted)
                .child("You have no collections on Wallhaven yet.")
                .into_any_element(),
            ImportState::Ready(list) => v_flex()
                .gap_1()
                .children(list.clone().into_iter().enumerate().map(|(i, c)| {
                    let label = c.label.clone();
                    h_flex()
                        .gap_2()
                        .h(px(30.))
                        .child(div().flex_1().min_w_0().truncate().text_sm().child(label))
                        .child(div().text_xs().text_color(p.dim).child(c.count.to_string()))
                        .child(
                            kit::Btn::new(("import-col", i))
                                .icon("download")
                                .ghost()
                                .sm()
                                .tooltip("Import as a gallery")
                                .on_click(cx.listener(move |this, _, _, cx| this.import_collection(&c, cx))),
                        )
                }))
                .into_any_element(),
        };
        Some(
            side_card_with("globe", "Wallhaven", p, |h| {
                h.child(kit::Btn::new("import-close").icon("close").ghost().sm().tooltip("Close").on_click(
                    cx.listener(|this, _, _, cx| {
                        this.import = None;
                        cx.notify();
                    }),
                ))
            })
            .child(
                v_flex()
                    .gap_1()
                    .child(div().text_xs().text_color(p.dim).child("Your username, which the API key doesn't reveal"))
                    .child(
                        div()
                            .h(px(32.))
                            .px_1()
                            .rounded_md()
                            .bg(p.bg)
                            .border_1()
                            .border_color(p.border_strong)
                            .child(Input::new(&self.wh_user).appearance(false).small()),
                    ),
            )
            .child(body),
        )
    }

    /// The top bar on the Galleries tab.
    pub(super) fn render_gallery_bar(&mut self, bar: Div, cx: &mut Context<Self>) -> Div {
        let p = pal();
        let Some(g) = self.gallery.and_then(|id| self.galleries.get(id)).cloned() else {
            let field = bar_field("galleries-field", &[], p)
                .child(icon("images").size_4().flex_none().text_color(p.accent))
                .child(div().flex_none().font_weight(FontWeight::SEMIBOLD).child("Galleries"))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_sm()
                        .text_color(p.muted)
                        .child("Sets of wallpapers to rotate through"),
                );
            return bar.child(no_drag(field));
        };
        let id = g.id;
        let tints: Vec<Hsla> = g
            .items
            .iter()
            .take(8)
            .filter_map(|e| match e {
                Entry::Wallhaven(w) => w.colors.first().and_then(|c| super::parse_hex(c)),
                Entry::Local(path) => self.local_palettes.get(path).and_then(|c| c.first().copied()),
            })
            .collect();
        let work = self.gallery_work.get(&id).copied();
        let renaming = self.renaming == Some(id);

        let name: AnyElement = if renaming {
            div().w(px(220.)).flex_none().child(Input::new(&self.gallery_name).appearance(false)).into_any_element()
        } else {
            no_drag(
                div()
                    .id("gallery-name")
                    .flex_none()
                    .max_w(px(260.))
                    .truncate()
                    .px_1p5()
                    .rounded_md()
                    .font_weight(FontWeight::SEMIBOLD)
                    .cursor_pointer()
                    .hover(move |s| s.bg(with_alpha(p.elevated, 0.8)))
                    .tooltip(|window, cx| Tooltip::new("Rename").build(window, cx))
                    .child(g.name.clone())
                    .on_click(cx.listener(move |this, _, window, cx| this.start_rename(id, window, cx))),
            )
            .into_any_element()
        };
        let count = g.items.len();
        let field = bar_field("gallery-field", &tints, p)
            .child(icon(kind_icon(&g.source)).size_4().flex_none().text_color(p.accent))
            .child(name)
            .child(div().min_w_0().truncate().text_sm().text_color(p.muted).child(describe(&g.source)))
            .child(div().flex_1())
            .child(div().flex_none().text_xs().text_color(p.dim).child(match work {
                Some(GalleryWork::Downloading { done, total }) => format!("saving {done} of {total}"),
                Some(GalleryWork::Refreshing) => "refreshing".into(),
                None => format!("{count} wallpaper{}", if count == 1 { "" } else { "s" }),
            }));

        let rotating = self.cfg.rotate_source == RotateSource::Gallery(id) && self.cfg.rotate_minutes > 0;
        let minutes = if self.cfg.rotate_minutes > 0 { self.cfg.rotate_minutes } else { DEFAULT_ROTATE_MINUTES };
        let offline_tip = match (g.offline, work) {
            (_, Some(GalleryWork::Downloading { done, total })) => format!("Saving originals: {done} of {total}"),
            (true, _) => "Originals kept offline. Click to stop".into(),
            (false, _) => "Keep originals offline".into(),
        };
        let deleting = self.confirm_delete == Some(id);
        let actions = super::segmented_track(p)
            .flex_none()
            .when(g.source.refreshable(), |d| {
                d.child(
                    kit::Btn::new("gallery-refresh")
                        .icon("refresh-cw")
                        .ghost()
                        .sm()
                        .loading(work == Some(GalleryWork::Refreshing))
                        .tooltip(if matches!(g.source, Source::Smart { .. }) {
                            "Run the search again"
                        } else {
                            "Fetch the collection again"
                        })
                        .on_click(cx.listener(move |this, _, _, cx| this.refresh_gallery(id, cx))),
                )
            })
            .child(
                kit::Btn::new("gallery-edit")
                    .icon("sliders-horizontal")
                    .ghost()
                    .sm()
                    .tooltip(match g.source {
                        Source::Smart { .. } => "Edit the search behind this gallery",
                        Source::Wallhaven { .. } => "Edit what this collection imports",
                        Source::Manual => "Rename",
                    })
                    .on_click(cx.listener(move |this, _, window, cx| this.open_gallery_editor(id, window, cx))),
            )
            .child(
                kit::Btn::new("gallery-offline")
                    .icon("hard-drive-download")
                    .ghost()
                    .sm()
                    .selected(g.offline)
                    .tooltip(offline_tip)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let on = this.galleries.get(id).is_some_and(|g| g.offline);
                        this.set_offline(id, !on, cx);
                    })),
            )
            .child(
                kit::Btn::new("gallery-rotate")
                    .icon("shuffle")
                    .ghost()
                    .sm()
                    .selected(rotating)
                    .tooltip(if rotating {
                        "Stop rotating".to_owned()
                    } else {
                        format!("Rotate from this gallery every {minutes} min")
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_rotation(id, cx))),
            )
            .child(
                kit::Btn::new("gallery-play")
                    .icon("play")
                    .ghost()
                    .sm()
                    .disabled(count == 0 || self.rotating)
                    .tooltip("Put up a set from this gallery now")
                    .on_click(cx.listener(move |this, _, _, cx| this.rotate_from(RotateSource::Gallery(id), cx))),
            )
            .child(div().w(px(1.)).h(px(16.)).mx_0p5().bg(p.border_strong))
            .child(if deleting {
                kit::Btn::new("gallery-delete")
                    .label("Delete?")
                    .danger()
                    .sm()
                    .on_click(cx.listener(move |this, _, _, cx| this.delete_gallery(id, cx)))
            } else {
                kit::Btn::new("gallery-delete").icon("trash").ghost().sm().tooltip("Delete gallery").on_click(
                    cx.listener(move |this, _, _, cx| {
                        this.confirm_delete = Some(id);
                        cx.notify();
                    }),
                )
            });

        bar.child(no_drag(field)).child(no_drag(actions)).child(no_drag(self.render_view_toggles(cx)))
    }

    /// What the Galleries tab shows with nothing to list.
    pub(super) fn render_gallery_empty(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = pal();
        let shell = |icon_name: &str, title: &str, sub: &str| {
            v_flex()
                .flex_1()
                .min_h_0()
                .items_center()
                .pt(px(140.))
                .px_8()
                .gap_4()
                .child(
                    div()
                        .size(px(64.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(with_alpha(p.accent, 0.12))
                        .border_1()
                        .border_color(with_alpha(p.accent, 0.35))
                        .child(icon(icon_name).size(px(26.)).text_color(p.accent)),
                )
                .child(
                    v_flex()
                        .items_center()
                        .gap_1()
                        .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child(title.to_owned()))
                        .child(div().max_w(px(440.)).text_sm().text_color(p.dim).child(sub.to_owned())),
                )
        };

        let Some(g) = self.gallery.and_then(|id| self.galleries.get(id)) else {
            let has_key = self.client.has_key();
            return shell(
                "images",
                "No galleries yet",
                "Collect wallpapers into galleries, then let auto-rotate play through them.",
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(kit::Btn::new("empty-new").label("New gallery").icon("plus").primary().on_click(
                        cx.listener(|this, _, _, cx| {
                            let id = this.new_gallery(None, cx);
                            this.open_gallery(id, cx);
                        }),
                    ))
                    .when(has_key, |d| {
                        d.child(
                            kit::Btn::new("empty-import")
                                .label("Import from Wallhaven")
                                .icon("download")
                                .on_click(cx.listener(|this, _, _, cx| this.load_collections(cx))),
                        )
                    }),
            )
            .into_any_element();
        };

        if self.gallery_work.get(&g.id) == Some(&GalleryWork::Refreshing) {
            return v_flex()
                .flex_1()
                .min_h_0()
                .pt(px(GRID_PAD))
                .gap(px(super::GRID_GAP))
                .overflow_hidden()
                .children((0..4).map(|r| self.skeleton_row(r)))
                .into_any_element();
        }
        if g.source == Source::Manual {
            shell(
                "images",
                "Nothing here yet",
                "Drag wallpapers onto this gallery in the sidebar, or save one with the bookmark button.",
            )
            .child(
                h_flex()
                    .gap_1p5()
                    .text_xs()
                    .text_color(p.dim)
                    .child("Right-click any card to")
                    .child(keycap("save", p)),
            )
            .into_any_element()
        } else {
            let id = g.id;
            shell("search-x", "Nothing came back", "The search or collection is empty for your current purity filter.")
                .child(
                    kit::Btn::new("empty-refresh")
                        .label("Try again")
                        .icon("refresh-cw")
                        .on_click(cx.listener(move |this, _, _, cx| this.refresh_gallery(id, cx))),
                )
                .into_any_element()
        }
    }

    /// Bookmark button: opens the picker for `item`.
    pub(super) fn gallery_button(&self, id: &'static str, item: &Item, cx: &mut Context<Self>) -> kit::Btn {
        let key = entry_of(item).key();
        let saved = self.galleries.list.iter().any(|g| g.source == Source::Manual && g.contains(&key));
        let item = item.clone();
        kit::Btn::new(id)
            .icon("bookmark")
            .ghost()
            .selected(saved)
            .tooltip(if saved { "In a gallery" } else { "Save to gallery" })
            .on_click(cx.listener(move |this, ev: &ClickEvent, _, cx| {
                this.picker = Some(Picker { item: item.clone(), at: ev.position() });
                cx.notify();
            }))
    }

    /// The save-to-gallery popover.
    pub(super) fn render_picker(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let p = pal();
        let picker = self.picker.as_ref()?;
        let (item, at) = (picker.item.clone(), picker.at);
        let key = entry_of(&item).key();
        let rows = self.manual_galleries().into_iter().map(|(id, name, n)| {
            let on = self.galleries.get(id).is_some_and(|g| g.contains(&key));
            let item = item.clone();
            h_flex()
                .id(("pick", id))
                .gap_2()
                .px_2()
                .h(px(32.))
                .rounded_md()
                .cursor_pointer()
                .hover(move |s| s.bg(p.surface2))
                .child(
                    div().flex_none().size(px(16.)).flex().items_center().justify_center().rounded_sm().border_1().map(
                        |d| {
                            if on {
                                d.bg(p.accent)
                                    .border_color(p.accent)
                                    .child(icon("check").size_3().text_color(super::on_accent(p.accent)))
                            } else {
                                d.border_color(p.border_strong)
                            }
                        },
                    ),
                )
                .child(div().flex_1().min_w_0().truncate().text_sm().child(name))
                .child(div().text_xs().text_color(p.dim).child(n.to_string()))
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_in_gallery(id, &item, cx)))
        });
        let new_item = item.clone();
        let panel = v_flex()
            .id("gallery-picker")
            .w(px(248.))
            .p_1p5()
            .gap_0p5()
            .rounded_xl()
            .bg(p.elevated)
            .border_1()
            .border_color(p.border_strong)
            .shadow_lg()
            .occlude()
            .child(div().px_2().py_1().text_xs().text_color(p.dim).child("Save to gallery"))
            .children(rows)
            .child(div().h(px(1.)).my_1().bg(p.border))
            .child(
                h_flex()
                    .id("pick-new")
                    .gap_2()
                    .px_2()
                    .h(px(32.))
                    .rounded_md()
                    .cursor_pointer()
                    .text_sm()
                    .text_color(p.muted)
                    .hover(move |s| s.bg(p.surface2).text_color(p.fg))
                    .child(icon("plus").size_4())
                    .child("New gallery")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let id = this.new_gallery(Some(new_item.clone()), cx);
                        let name = this.galleries.get(id).map(|g| g.name.clone()).unwrap_or_default();
                        this.set_status(format!("Added to {name}"), false, cx);
                    })),
            )
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.picker = None;
                cx.notify();
            }));
        Some(deferred(anchored().position(at).snap_to_window().child(panel)).with_priority(2).into_any_element())
    }

    /// While a card is dragged anywhere but the Galleries tab, galleries
    /// rise from the bottom of the sidebar as drop targets.
    pub(super) fn render_drop_dock(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !cx.has_active_drag() || self.tab == Tab::Galleries {
            return None;
        }
        let p = pal();
        let targets = |id: ElementId, label: String, lead: &'static str, p: Pal| {
            h_flex()
                .id(id)
                .gap_2()
                .px_2p5()
                .h(px(34.))
                .rounded_lg()
                .border_1()
                .border_color(p.border)
                .bg(with_alpha(p.surface2, 0.9))
                .text_sm()
                .child(icon(lead).size_4().text_color(p.accent))
                .child(div().flex_1().min_w_0().truncate().child(label))
                .drag_over::<drag::CardDrag>(move |s, _, _, _| s.bg(with_alpha(p.accent, 0.22)).border_color(p.accent))
        };
        let rows = self.manual_galleries().into_iter().take(6).map(|(id, name, _)| {
            targets(("dock", id).into(), name, "images", p)
                .on_drop(cx.listener(move |this, drag: &drag::CardDrag, _, cx| {
                    this.add_to_gallery(id, vec![drag.item.clone()], cx);
                }))
                .into_any_element()
        });
        let new_row = targets("dock-new".into(), "New gallery".into(), "plus", p).on_drop(cx.listener(
            |this, drag: &drag::CardDrag, _, cx| {
                let id = this.new_gallery(Some(drag.item.clone()), cx);
                let name = this.galleries.get(id).map(|g| g.name.clone()).unwrap_or_default();
                this.set_status(format!("Added to {name}"), false, cx);
            },
        ));
        Some(
            v_flex()
                .absolute()
                .left_0()
                .right_0()
                .bottom_0()
                .p_3()
                .gap_1p5()
                .bg(with_alpha(p.surface, 0.97))
                .border_t_1()
                .border_color(with_alpha(p.accent, 0.5))
                .shadow_lg()
                .child(
                    h_flex()
                        .gap_1p5()
                        .pb_0p5()
                        .text_xs()
                        .text_color(p.dim)
                        .child(icon("bookmark").size_3())
                        .child("Drop into a gallery"),
                )
                .children(rows)
                .child(new_row)
                .with_animation(
                    "dock-rise",
                    Animation::new(std::time::Duration::from_millis(180)).with_easing(ease_out_quint()),
                    |d, t| d.mb(px(-24. * (1. - t))).opacity(t),
                )
                .into_any_element(),
        )
    }
}
