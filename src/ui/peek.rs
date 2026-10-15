//! Hold Space: the selected wallpaper goes onto your real monitors. Let go
//! and they snap back to what was there.

use std::collections::BTreeMap;
use std::path::PathBuf;

use gpui_kit::component::h_flex;
use gpui_kit::*;

use super::{FullState, Item, Muralist, Tab, full_key, icon, pal, with_alpha};
use crate::config::cache_dir;
use crate::span;
use crate::thumbs::remote_cache_path;
use crate::wallhaven::Wallpaper;

/// Images at least this much wider than tall are spanned across monitors.
const SPAN_RATIO: f32 = 2.4;

enum PeekSource {
    Image { path: PathBuf, span: bool },
    Map(BTreeMap<String, PathBuf>),
}

pub(super) struct Peek {
    /// What was on each monitor before, once read back from the desktop.
    restore: Option<BTreeMap<String, PathBuf>>,
    /// Space came back up before the peek finished applying.
    released: bool,
}

impl Muralist {
    /// Whether a text field has focus (Space should type, not peek).
    pub(super) fn typing(&self, window: &Window, cx: &App) -> bool {
        [&self.query, &self.key_input, &self.dir_input]
            .iter()
            .any(|input| input.read(cx).focus_handle(cx).contains_focused(window, cx))
    }

    /// The downloaded original, if it's on disk yet.
    pub(super) fn original_on_disk(&self, w: &Wallpaper) -> Option<PathBuf> {
        let saved = self.cfg.download_dir.join(w.file_name());
        if saved.is_file() {
            return Some(saved);
        }
        if let Some(FullState::Ready(path)) = self.full.get(&w.id) {
            return Some(path.clone());
        }
        let cached = remote_cache_path(&full_key(w), &w.path);
        cached.is_file().then_some(cached)
    }

    pub(super) fn peek_start(&mut self, cx: &mut Context<Self>) {
        if self.peek.is_some() || self.monitors.is_empty() {
            return;
        }
        // Over a history entry: peek that whole set.
        if self.tab == Tab::History {
            if let Some(map) = self.history_peek_map().filter(|m| !m.is_empty()) {
                self.peek_map(map, cx);
            }
            return;
        }
        let Some(item) = self.selected.clone() else {
            self.set_status("Select a wallpaper, then hold Space to peek", false, cx);
            return;
        };
        let path = match &item {
            Item::Remote(w) => self.original_on_disk(w),
            Item::Local(path) => Some(path.clone()),
        };
        let Some(path) = path else {
            self.set_status("Original still downloading; peek in a moment", false, cx);
            return;
        };
        let span = self.monitors.len() > 1
            && self.selected_dims.is_some_and(|(w, h)| w as f32 / h.max(1) as f32 >= SPAN_RATIO);
        self.peek_with(PeekSource::Image { path, span }, cx);
    }

    fn peek_map(&mut self, map: BTreeMap<String, PathBuf>, cx: &mut Context<Self>) {
        self.peek_with(PeekSource::Map(map), cx);
    }

    fn peek_with(&mut self, source: PeekSource, cx: &mut Context<Self>) {
        let monitors = self.monitors.clone();
        let backend = self.backend.clone();
        self.peek = Some(Peek { restore: None, released: false });
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let restore = backend.current_desktop(&monitors)?;
                    let map: BTreeMap<String, PathBuf> = match source {
                        PeekSource::Map(map) => map,
                        PeekSource::Image { path, span: true } => {
                            span::slice(&path, &monitors, &cache_dir().join("span"))?
                        }
                        PeekSource::Image { path, span: false } => {
                            monitors.iter().map(|m| (m.name.clone(), path.clone())).collect()
                        }
                    };
                    backend.set_desktop(&map, &monitors)?;
                    anyhow::Ok(restore)
                })
                .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(restore) => {
                        let released = this.peek.as_ref().is_none_or(|p| p.released);
                        if released {
                            this.restore_peek(restore, cx);
                        } else if let Some(peek) = &mut this.peek {
                            peek.restore = Some(restore);
                        }
                    }
                    Err(e) => {
                        this.peek = None;
                        this.set_status(format!("Peek failed: {e:#}"), true, cx);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn peek_end(&mut self, cx: &mut Context<Self>) {
        match self.peek.take() {
            Some(Peek { restore: Some(restore), .. }) => self.restore_peek(restore, cx),
            // Still applying: restore as soon as it lands.
            Some(Peek { restore: None, .. }) => {
                self.peek = Some(Peek { restore: None, released: true });
            }
            None => {}
        }
        cx.notify();
    }

    fn restore_peek(&mut self, restore: BTreeMap<String, PathBuf>, cx: &mut Context<Self>) {
        self.peek = None;
        let restore = if restore.is_empty() { self.applied.desktop.clone() } else { restore };
        if restore.is_empty() {
            return;
        }
        let monitors = self.monitors.clone();
        let backend = self.backend.clone();
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { backend.set_desktop(&restore, &monitors) }).await;
            if let Err(e) = result {
                this.update(cx, |this, cx| this.set_status(format!("Restoring after peek failed: {e:#}"), true, cx))
                    .ok();
            }
        })
        .detach();
    }

    pub(super) fn render_peek_badge(&self) -> Option<AnyElement> {
        let peek = self.peek.as_ref()?;
        let p = pal();
        let text = if peek.restore.is_some() {
            "Peeking on your desktop  ·  release Space to go back"
        } else {
            "Putting it on your monitors..."
        };
        Some(
            h_flex()
                .absolute()
                .top(px(70.))
                .left_0()
                .right_0()
                .justify_center()
                .child(
                    h_flex()
                        .gap_2p5()
                        .px_4()
                        .h(px(42.))
                        .rounded_full()
                        .bg(with_alpha(p.elevated, 0.95))
                        .border_1()
                        .border_color(with_alpha(p.accent, 0.6))
                        .text_sm()
                        .child(icon("eye").size_4().text_color(p.accent))
                        .child(text)
                        .with_animation(
                            "peek-badge",
                            Animation::new(std::time::Duration::from_millis(1400))
                                .repeat()
                                .with_easing(pulsating_between(0.75, 1.)),
                            |d, t| d.opacity(t),
                        ),
                )
                .into_any_element(),
        )
    }
}
