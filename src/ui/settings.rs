//! Settings: a two-pane window. Sections on the left, the chosen section's
//! grouped rows on the right. Everything applies as soon as it changes; the
//! API key and folder fields get their own Save once edited.

use std::path::PathBuf;
use std::time::Instant;

use gpui_kit::component::input::Input;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    KeyTest, Muralist, Pal, Reach, dir_size, hex, icon, kit, opt, opt_grid, pal, segment, segmented_track, switch,
    with_alpha,
};
use crate::config::{LockMode, RotateOrder, RotateSource, Spoiler, cache_dir};
use crate::wallhaven::Client;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Section {
    #[default]
    Account,
    Appearance,
    Rotation,
    Downloads,
    Desktop,
    Lock,
    Data,
}

impl Section {
    fn label(self) -> &'static str {
        match self {
            Section::Account => "Account",
            Section::Appearance => "Appearance",
            Section::Rotation => "Auto-rotate",
            Section::Downloads => "Downloads",
            Section::Desktop => "Desktop",
            Section::Lock => "Lock screen",
            Section::Data => "Data",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Section::Account => "key-round",
            Section::Appearance => "palette",
            Section::Rotation => "shuffle",
            Section::Downloads => "folder",
            Section::Desktop => "monitor",
            Section::Lock => "lock",
            Section::Data => "database",
        }
    }

    fn blurb(self) -> &'static str {
        match self {
            Section::Account => "Your Wallhaven API key and username.",
            Section::Appearance => "How muralist looks and what it hides.",
            Section::Rotation => "Fresh wallpapers on a timer while muralist is open.",
            Section::Downloads => "Where full-size wallpapers are saved.",
            Section::Desktop => "How muralist reaches your screens.",
            Section::Lock => "How the lock screen is set.",
            Section::Data => "Filters, prefetching and the cache.",
        }
    }
}

/// A titled row: label and description on the left, a control on the right.
fn row(title: &str, desc: impl Into<SharedString>, control: impl IntoElement, p: Pal) -> Div {
    h_flex()
        .justify_between()
        .gap_6()
        .px_4()
        .py_3()
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(title.to_owned()))
                .child(div().text_xs().text_color(p.dim).child(desc.into())),
        )
        .child(div().flex_none().child(control))
}

/// A row that is a switch: the whole row toggles.
fn switch_row(id: &'static str, title: &str, desc: &str, on: bool, p: Pal) -> Stateful<Div> {
    row(title, desc.to_owned(), switch(on, p), p)
        .id(id)
        .cursor_pointer()
        .hover(move |s| s.bg(with_alpha(p.elevated, 0.5)))
}

/// A row whose control sits under its text, full width.
fn stacked(title: &str, desc: &str, control: impl IntoElement, p: Pal) -> Div {
    v_flex()
        .gap_2p5()
        .px_4()
        .py_3()
        .child(
            v_flex()
                .gap_0p5()
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(title.to_owned()))
                .child(div().text_xs().text_color(p.dim).child(desc.to_owned())),
        )
        .child(control)
}

/// Rows in one card, divided by hairlines.
fn group(rows: Vec<AnyElement>, p: Pal) -> Div {
    v_flex().rounded_xl().overflow_hidden().bg(with_alpha(p.surface2, 0.82)).border_1().border_color(p.border).children(
        rows.into_iter()
            .enumerate()
            .map(move |(i, r)| div().when(i > 0, |d| d.border_t_1().border_color(p.border)).child(r)),
    )
}

/// A text field in the settings' style.
fn field_box(input: impl IntoElement, p: Pal) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .h(px(34.))
        .flex()
        .items_center()
        .px_1()
        .rounded_lg()
        .bg(p.bg)
        .border_1()
        .border_color(p.border_strong)
        .child(div().flex_1().child(input))
}

/// "512 MB", "1.8 GB".
fn mb_or_gb(bytes: u64) -> String {
    let mb = super::mib(bytes);
    if mb >= 1024. { format!("{:.1} GB", mb / 1024.) } else { format!("{mb:.0} MB") }
}

fn save_cfg(this: &Muralist) {
    if let Err(e) = this.cfg.save() {
        eprintln!("saving config: {e:#}");
    }
}

impl Muralist {
    pub(super) fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let key = self.cfg.api_key.clone();
        let dir = self.cfg.download_dir.display().to_string();
        let user = self.cfg.wallhaven_user.clone();
        self.key_input.update(cx, |s, cx| s.set_value(key, window, cx));
        self.dir_input.update(cx, |s, cx| s.set_value(dir, window, cx));
        self.wh_user.update(cx, |s, cx| s.set_value(user, window, cx));
        self.key_test = KeyTest::Idle;
        self.cache_size = dir_size(&cache_dir());
        self.settings_open = true;
        self.focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn close_settings(&mut self, cx: &mut Context<Self>) {
        self.settings_open = false;
        cx.notify();
    }

    fn test_key(&mut self, cx: &mut Context<Self>) {
        let key = self.key_input.read(cx).value().trim().to_string();
        if key.is_empty() {
            self.key_test = KeyTest::Failed("Enter a key first".into());
            cx.notify();
            return;
        }
        self.key_test = KeyTest::Testing;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { Client::new(&key)?.settings() }).await;
            this.update(cx, |this, cx| {
                this.key_test = match result {
                    Ok(s) => {
                        let purity: Vec<&str> = s
                            .purity
                            .iter()
                            .map(|p| crate::wallhaven::Purity::parse(p).map_or("?", crate::wallhaven::Purity::label))
                            .collect();
                        KeyTest::Ok(format!("Key works  ·  account allows {}", purity.join(", ")))
                    }
                    Err(e) => KeyTest::Failed(format!("{e:#}")),
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn apply_key(&mut self, cx: &mut Context<Self>) {
        let key = self.key_input.read(cx).value().trim().to_string();
        if key == self.cfg.api_key {
            return;
        }
        self.cfg.api_key = key;
        save_cfg(self);
        match Client::new(self.cfg.effective_api_key()) {
            Ok(client) => self.client = client,
            Err(e) => {
                self.set_status(format!("HTTP client: {e:#}"), true, cx);
                return;
            }
        }
        self.account = None;
        self.info.clear();
        if self.client.has_key() {
            self.load_account(cx);
        } else {
            self.restore_filters();
            self.search(true, cx);
        }
        self.set_status("API key saved", false, cx);
    }

    fn apply_dir(&mut self, cx: &mut Context<Self>) {
        let dir = self.dir_input.read(cx).value().trim().to_string();
        if dir.is_empty() {
            return;
        }
        self.cfg.download_dir = match dir.strip_prefix("~/") {
            Some(rest) => dirs::home_dir().unwrap_or_default().join(rest),
            None => PathBuf::from(dir),
        };
        save_cfg(self);
        self.refresh_library();
        self.set_status("Download folder saved", false, cx);
    }

    /// The Wallhaven username field commits on Enter or when it loses focus.
    pub(super) fn commit_username(&mut self, cx: &mut Context<Self>) {
        let user = self.wh_user.read(cx).value().trim().trim_start_matches('@').to_owned();
        if user != self.cfg.wallhaven_user {
            self.cfg.wallhaven_user = user;
            save_cfg(self);
        }
    }

    fn clear_cache(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for sub in ["thumbs", "span"] {
            let _ = std::fs::remove_dir_all(cache_dir().join(sub));
        }
        self.thumbs.clear();
        self.requested.clear();
        self.preview_cache.update(cx, |cache, cx| cache.clear(window, cx));
        self.cache_size = dir_size(&cache_dir());
        self.set_status("Cache cleared", false, cx);
    }

    fn settings_sections(&self) -> Vec<Section> {
        let per_monitor_lock = self.backend.lock().is_some_and(|w| w.reach() == Reach::PerMonitor);
        [
            Section::Account,
            Section::Appearance,
            Section::Rotation,
            Section::Downloads,
            Section::Desktop,
            Section::Lock,
            Section::Data,
        ]
        .into_iter()
        .filter(|s| *s != Section::Lock || per_monitor_lock)
        .collect()
    }

    pub(super) fn render_settings(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let p = pal();
        let sections = self.settings_sections();
        let current = if sections.contains(&self.settings_section) { self.settings_section } else { Section::Account };

        let nav = v_flex()
            .flex_none()
            .w(px(208.))
            .h_full()
            .p_3()
            .gap_0p5()
            .bg(p.surface)
            .border_r_1()
            .border_color(p.border)
            .child(
                h_flex()
                    .gap_2p5()
                    .px_2()
                    .h(px(44.))
                    .mb_1()
                    .child(icon("settings").size_4().text_color(p.accent))
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Settings")),
            )
            .children(sections.into_iter().map(|s| {
                let on = s == current;
                h_flex()
                    .id(s.label())
                    .gap_2p5()
                    .px_2p5()
                    .h(px(34.))
                    .rounded_lg()
                    .cursor_pointer()
                    .text_sm()
                    .map(|d| {
                        if on {
                            d.bg(with_alpha(p.accent, 0.12)).text_color(p.fg).font_weight(FontWeight::MEDIUM)
                        } else {
                            d.text_color(p.muted).hover(move |st| st.bg(with_alpha(p.elevated, 0.7)).text_color(p.fg))
                        }
                    })
                    .child(icon(s.icon()).size_4().text_color(if on { p.accent } else { p.dim }))
                    .child(s.label())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.settings_section = s;
                        cx.notify();
                    }))
            }));

        let body: Vec<AnyElement> = match current {
            Section::Account => self.settings_account(cx),
            Section::Appearance => self.settings_appearance(cx),
            Section::Rotation => self.settings_rotation(cx),
            Section::Downloads => self.settings_downloads(cx),
            Section::Desktop => self.settings_desktop(cx),
            Section::Lock => self.settings_lock(cx),
            Section::Data => self.settings_data(cx),
        };

        let pane = v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(
                h_flex()
                    .flex_none()
                    .h(px(68.))
                    .px_6()
                    .gap_3()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(div().text_lg().font_weight(FontWeight::SEMIBOLD).child(current.label()))
                            .child(div().text_xs().text_color(p.dim).child(current.blurb())),
                    )
                    .child(
                        kit::Btn::new("settings-close")
                            .icon("close")
                            .ghost()
                            .sm()
                            .tooltip("Close (Esc)")
                            .on_click(cx.listener(|this, _, _, cx| this.close_settings(cx))),
                    ),
            )
            .child(
                div()
                    .id(("settings-body", current as usize))
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(v_flex().gap_4().px_6().pb_6().children(body)),
            );

        let window_card = h_flex()
            .id("settings-card")
            .w(px(860.))
            .h(px(600.))
            .max_h(relative(0.9))
            .rounded_xl()
            .overflow_hidden()
            .bg(p.bg)
            .border_1()
            .border_color(p.border_strong)
            .shadow_lg()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(nav)
            .child(pane);

        div()
            .id("settings-overlay")
            .track_focus(&self.focus)
            .occlude()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(hsla(0., 0., 0., 0.6))
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                if ev.keystroke.key == "escape" {
                    this.close_settings(cx);
                    cx.stop_propagation();
                }
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.close_settings(cx)))
            .child(window_card)
            .into_any_element()
    }

    fn settings_account(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let p = pal();
        let typed = self.key_input.read(cx).value().trim().to_string();
        let dirty = typed != self.cfg.api_key;
        let key_status = match &self.key_test {
            KeyTest::Idle => None,
            KeyTest::Testing => Some(
                h_flex()
                    .gap_2()
                    .text_xs()
                    .text_color(p.muted)
                    .child(Spinner::new().xsmall().color(p.accent))
                    .child("Checking key..."),
            ),
            KeyTest::Ok(msg) => Some(
                h_flex().gap_2().text_xs().text_color(p.ok).child(icon("circle-check").size_3p5()).child(msg.clone()),
            ),
            KeyTest::Failed(msg) => Some(
                h_flex().gap_2().text_xs().text_color(p.error).child(icon("circle-x").size_3p5()).child(msg.clone()),
            ),
        };
        let key_controls = v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .child(field_box(Input::new(&self.key_input).appearance(false).mask_toggle(), p))
                    .child(
                        kit::Btn::new("test-key")
                            .label("Test")
                            .on_click(cx.listener(|this, _, _, cx| this.test_key(cx))),
                    )
                    .when(dirty, |d| {
                        d.child(
                            kit::Btn::new("save-key")
                                .label("Save")
                                .primary()
                                .on_click(cx.listener(|this, _, _, cx| this.apply_key(cx))),
                        )
                    }),
            )
            .children(key_status)
            .when(self.cfg.env_api_key.is_some(), |d| {
                d.child(
                    h_flex()
                        .gap_2()
                        .text_xs()
                        .text_color(hex(0xf59e0b))
                        .child(icon("triangle-alert").size_3p5())
                        .child("WALLHAVEN_API_KEY is set in the environment and takes priority."),
                )
            });

        let account_state = match &self.account {
            Some(s) => {
                let purity: Vec<&str> = s
                    .purity
                    .iter()
                    .map(|p| crate::wallhaven::Purity::parse(p).map_or("?", crate::wallhaven::Purity::label))
                    .collect();
                format!("Defaults loaded  ·  allows {}", purity.join(", "))
            }
            None if self.client.has_key() => "Loading your account defaults...".into(),
            None => "No key: SFW results and Wallhaven's defaults only.".into(),
        };

        vec![
            group(
                vec![
                    row(
                        "Wallhaven account",
                        account_state,
                        kit::Btn::new("get-key")
                            .label("Get a key")
                            .icon("external-link")
                            .sm()
                            .on_click(|_, _, cx| cx.open_url("https://wallhaven.cc/settings/account")),
                        p,
                    )
                    .into_any_element(),
                    stacked(
                        "API key",
                        "Unlocks NSFW results and loads your account's default filters. Read-only.",
                        key_controls,
                        p,
                    )
                    .into_any_element(),
                ],
                p,
            )
            .into_any_element(),
            group(
                vec![
                    stacked(
                        "Username",
                        "Needed to import your collections as galleries; the API key alone doesn't reveal it.",
                        field_box(Input::new(&self.wh_user).appearance(false), p),
                        p,
                    )
                    .into_any_element(),
                ],
                p,
            )
            .into_any_element(),
        ]
    }

    fn settings_appearance(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let p = pal();
        let spoiler = self.cfg.spoiler;
        let spoiler_seg = |id: &'static str, label: &'static str, v: Spoiler| {
            segment(id, label, spoiler == v, p).on_click(cx.listener(move |this, _, _, cx| {
                this.cfg.spoiler = v;
                save_cfg(this);
                cx.notify();
            }))
        };
        let mut hiding = vec![
            stacked(
                "Hide until revealed",
                "Cards behind a pixel mosaic until you hover or select them.",
                segmented_track(p)
                    .child(spoiler_seg("sp-off", "Off", Spoiler::Off))
                    .child(spoiler_seg("sp-nsfw", "NSFW", Spoiler::Nsfw))
                    .child(spoiler_seg("sp-sketchy", "NSFW + Sketchy", Spoiler::Sketchy)),
                p,
            )
            .into_any_element(),
        ];
        if spoiler != Spoiler::Off {
            hiding.push(
                switch_row(
                    "spoiler-hover",
                    "Reveal on hover",
                    "Off: hidden cards stay hidden until you click them.",
                    self.cfg.spoiler_hover,
                    p,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.cfg.spoiler_hover = !this.cfg.spoiler_hover;
                    save_cfg(this);
                    cx.notify();
                }))
                .into_any_element(),
            );
        }
        vec![
            group(
                vec![
                    switch_row(
                        "adaptive-toggle",
                        "Adaptive theme",
                        "The whole app takes on the selected wallpaper's colors.",
                        self.cfg.adaptive_theme,
                        p,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.cfg.adaptive_theme = !this.cfg.adaptive_theme;
                        save_cfg(this);
                        cx.notify();
                    }))
                    .into_any_element(),
                    switch_row(
                        "rainbow-toggle",
                        "Rainbow order",
                        "Sort results and galleries by color instead of their usual order.",
                        self.cfg.rainbow,
                        p,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.cfg.rainbow = !this.cfg.rainbow;
                        save_cfg(this);
                        cx.notify();
                    }))
                    .into_any_element(),
                ],
                p,
            )
            .into_any_element(),
            group(hiding, p).into_any_element(),
        ]
    }

    fn settings_rotation(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let p = pal();
        let minutes = self.cfg.rotate_minutes;
        let interval = |id: &'static str, label: &'static str, value: u32| {
            segment(id, label, minutes == value, p).on_click(cx.listener(move |this, _, _, cx| {
                this.cfg.rotate_minutes = value;
                this.last_rotation = Instant::now();
                save_cfg(this);
                cx.notify();
            }))
        };
        let timer = group(
            vec![
                stacked(
                    "Every",
                    "Puts up a fresh, color-matched set on a timer.",
                    segmented_track(p)
                        .child(interval("rot-off", "Off", 0))
                        .child(interval("rot-15", "15 min", 15))
                        .child(interval("rot-30", "30 min", 30))
                        .child(interval("rot-60", "1 hour", 60))
                        .child(interval("rot-180", "3 hours", 180)),
                    p,
                )
                .into_any_element(),
                row(
                    "Now",
                    self.rotation_countdown().unwrap_or_else(|| "Auto-rotate is off".into()),
                    kit::Btn::new("rotate-now")
                        .label("Rotate now")
                        .icon("shuffle")
                        .sm()
                        .loading(self.rotating)
                        .disabled(self.monitors.is_empty())
                        .on_click(cx.listener(|this, _, _, cx| this.rotate_now(cx))),
                    p,
                )
                .into_any_element(),
            ],
            p,
        );

        let source = self.cfg.rotate_source;
        let src = |id: &'static str, label: &'static str, on: bool, to: RotateSource| {
            segment(id, label, on, p).on_click(cx.listener(move |this, _, _, cx| {
                this.cfg.rotate_source = to;
                this.cfg.rotate_cursor = 0;
                save_cfg(this);
                cx.notify();
            }))
        };
        let first_gallery =
            self.gallery.filter(|id| self.galleries.get(*id).is_some()).or(self.galleries.list.first().map(|g| g.id));
        let mut from_rows = vec![
            stacked(
                "Draw from",
                "Random results for your current filters, your downloaded library, or a gallery.",
                segmented_track(p)
                    .child(src(
                        "rot-src-search",
                        "Current filters",
                        source == RotateSource::Search,
                        RotateSource::Search,
                    ))
                    .child(src("rot-src-lib", "Library", source == RotateSource::Library, RotateSource::Library))
                    .when_some(first_gallery, |d, id| {
                        d.child(src(
                            "rot-src-gallery",
                            "Gallery",
                            matches!(source, RotateSource::Gallery(_)),
                            RotateSource::Gallery(id),
                        ))
                    }),
                p,
            )
            .into_any_element(),
        ];
        if let RotateSource::Gallery(current) = source {
            let picks = self.galleries.list.iter().map(|g| {
                let id = g.id;
                opt(("rot-gallery", id), g.name.clone(), id == current, None, p).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.cfg.rotate_source = RotateSource::Gallery(id);
                        this.cfg.rotate_cursor = 0;
                        save_cfg(this);
                        cx.notify();
                    },
                ))
            });
            let order = self.cfg.rotate_order;
            let order_seg = |id: &'static str, label: &'static str, o: RotateOrder| {
                segment(id, label, order == o, p).on_click(cx.listener(move |this, _, _, cx| {
                    this.cfg.rotate_order = o;
                    save_cfg(this);
                    cx.notify();
                }))
            };
            from_rows
                .push(stacked("Gallery", "Which gallery to play.", opt_grid(3).children(picks), p).into_any_element());
            from_rows.push(
                row(
                    "Order",
                    "Shuffle within the time of day, or play straight through.",
                    segmented_track(p)
                        .child(order_seg("rot-shuffle", "Shuffle", RotateOrder::Shuffle))
                        .child(order_seg("rot-in-order", "In order", RotateOrder::InOrder)),
                    p,
                )
                .into_any_element(),
            );
        }

        let mut options = vec![
            switch_row(
                "rot-time",
                "Match time of day",
                "Darker wallpapers at night, brighter ones during the day.",
                self.cfg.rotate_match_time,
                p,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.cfg.rotate_match_time = !this.cfg.rotate_match_time;
                save_cfg(this);
                cx.notify();
            }))
            .into_any_element(),
        ];
        if self.backend.lock().is_some() {
            options.push(
                switch_row(
                    "rot-lock",
                    "Lock screen too",
                    "Rotate the lock screen along with the desktop.",
                    self.cfg.rotate_lock,
                    p,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.cfg.rotate_lock = !this.cfg.rotate_lock;
                    save_cfg(this);
                    cx.notify();
                }))
                .into_any_element(),
            );
        }
        vec![timer.into_any_element(), group(from_rows, p).into_any_element(), group(options, p).into_any_element()]
    }

    fn settings_downloads(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let p = pal();
        let typed = self.dir_input.read(cx).value().trim().to_string();
        let dirty = !typed.is_empty() && typed != self.cfg.download_dir.display().to_string();
        let dir = self.cfg.download_dir.clone();
        let n = self.library.len();
        vec![
            group(
                vec![
                    stacked(
                        "Folder",
                        "Full-size wallpapers are saved here, and the Library shows what's in it.",
                        h_flex()
                            .gap_2()
                            .child(field_box(Input::new(&self.dir_input).appearance(false), p))
                            .child(
                                kit::Btn::new("settings-open-dir")
                                    .label("Open")
                                    .icon("folder-open")
                                    .on_click(move |_, _, cx| cx.open_url(&format!("file://{}", dir.display()))),
                            )
                            .when(dirty, |d| {
                                d.child(
                                    kit::Btn::new("save-dir")
                                        .label("Save")
                                        .primary()
                                        .on_click(cx.listener(|this, _, _, cx| this.apply_dir(cx))),
                                )
                            }),
                        p,
                    )
                    .into_any_element(),
                    row(
                        "In the folder",
                        format!("{n} wallpaper{}", if n == 1 { "" } else { "s" }),
                        kit::Btn::new("rescan").label("Rescan").icon("refresh-cw").sm().on_click(cx.listener(
                            |this, _, _, cx| {
                                this.refresh_library();
                                cx.notify();
                            },
                        )),
                        p,
                    )
                    .into_any_element(),
                ],
                p,
            )
            .into_any_element(),
        ]
    }

    /// Puts what's set back up, running the desktop's scripts again: after a
    /// Plasma restart, a wallpaper plugin change, or a screen that didn't
    /// take its picture.
    fn reapply(&mut self, cx: &mut Context<Self>) {
        self.desktop = self.applied.desktop.clone();
        self.lock = self.applied.lock.clone();
        let mut targets = Vec::new();
        if !self.desktop.is_empty() {
            targets.push(super::Target::Desktop);
        }
        if !self.lock.is_empty() && self.backend.lock().is_some() {
            targets.push(super::Target::Lock);
        }
        if targets.is_empty() {
            self.set_status("Nothing set yet to reapply", true, cx);
            return;
        }
        self.apply(targets, cx);
    }

    fn settings_desktop(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let p = pal();
        let mut screens: Vec<String> = self
            .monitors
            .iter()
            .map(|m| format!("{} {}x{}{}", m.name, m.px_width, m.px_height, if m.primary { " (primary)" } else { "" }))
            .collect();
        if screens.is_empty() {
            screens.push("none found".into());
        }
        let busy = self.busy > 0;
        vec![
            group(
                vec![
                    row(
                        "Connected to",
                        self.backend.name().to_owned(),
                        icon("circle-check").size_4().text_color(if self.backend.is_supported() {
                            p.ok
                        } else {
                            p.dim
                        }),
                        p,
                    )
                    .into_any_element(),
                    row("Screens", screens.join("  ·  "), div(), p).into_any_element(),
                ],
                p,
            )
            .into_any_element(),
            group(
                vec![
                    row(
                        "Reapply",
                        "Put your current wallpapers back up on the desktop and lock screen, running the scripts \
                         again: after Plasma restarts, a wallpaper plugin changes, or a screen didn't take its \
                         picture.",
                        kit::Btn::new("set-reapply")
                            .label("Reapply")
                            .icon("refresh-cw")
                            .loading(busy)
                            .on_click(cx.listener(|this, _, _, cx| this.reapply(cx))),
                        p,
                    )
                    .into_any_element(),
                ],
                p,
            )
            .into_any_element(),
        ]
    }

    fn settings_lock(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let p = pal();
        let mode = self.cfg.lock_mode;
        vec![
            group(
                vec![
                    stacked(
                        "Style",
                        "Per display gives each monitor its own lock-screen image.",
                        segmented_track(p)
                            .child(
                                segment("set-lm-per", "Per display", mode == LockMode::PerMonitor, p).on_click(
                                    cx.listener(|this, _, _, cx| this.set_lock_mode(LockMode::PerMonitor, cx)),
                                ),
                            )
                            .child(
                                segment("set-lm-single", "One image (primary)", mode == LockMode::Single, p)
                                    .on_click(cx.listener(|this, _, _, cx| this.set_lock_mode(LockMode::Single, cx))),
                            ),
                        p,
                    )
                    .into_any_element(),
                ],
                p,
            )
            .into_any_element(),
        ]
    }

    fn settings_data(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let p = pal();
        let saved = self.cfg.filters.count();
        let (hits, started) = (self.precog.hits, self.precog.started);
        vec![
            group(
                vec![
                    row(
                        "Saved filters",
                        if saved == 0 {
                            "Using your account defaults".to_owned()
                        } else {
                            format!("{saved} filter{} differ from your account", if saved == 1 { "" } else { "s" })
                        },
                        kit::Btn::new("settings-reset")
                            .label("Reset")
                            .icon("rotate-ccw")
                            .sm()
                            .disabled(saved == 0)
                            .on_click(cx.listener(|this, _, _, cx| this.reset_filters(cx))),
                        p,
                    )
                    .into_any_element(),
                    switch_row(
                        "precog-toggle",
                        "Precognition",
                        &if started > 0 {
                            format!(
                                "Preloads the wallpaper your cursor is heading for. {hits} of {started} guesses used this session."
                            )
                        } else {
                            "Preloads the wallpaper your cursor is heading for, before you click.".to_owned()
                        },
                        self.cfg.precog,
                        p,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.cfg.precog = !this.cfg.precog;
                        save_cfg(this);
                        cx.notify();
                    }))
                    .into_any_element(),
                ],
                p,
            )
            .into_any_element(),
            group(
                vec![
                    row(
                        "Cache",
                        {
                            let used = mb_or_gb(self.cache_size);
                            match self.cfg.cache_limit_mb {
                                0 => format!("{used} of thumbnails and originals, no limit"),
                                mb => format!("{used} of {}  ·  least recently used goes first", mb_or_gb(u64::from(mb) * 1_048_576)),
                            }
                        },
                        kit::Btn::new("settings-clear-cache")
                            .label("Clear")
                            .icon("trash")
                            .danger()
                            .sm()
                            .on_click(cx.listener(|this, _, window, cx| this.clear_cache(window, cx))),
                        p,
                    )
                    .into_any_element(),
                    {
                        let limit = self.cfg.cache_limit_mb;
                        let opt_seg = |id: &'static str, label: &'static str, mb: u32| {
                            segment(id, label, limit == mb, p).on_click(cx.listener(move |this, _, _, cx| {
                                this.cfg.cache_limit_mb = mb;
                                save_cfg(this);
                                this.sweep_cache_now(cx);
                                cx.notify();
                            }))
                        };
                        row(
                            "Cache limit",
                            "Older thumbnails and cached originals are removed past this.",
                            segmented_track(p)
                                .child(opt_seg("cache-512", "512 MB", 512))
                                .child(opt_seg("cache-1g", "1 GB", 1024))
                                .child(opt_seg("cache-2g", "2 GB", 2048))
                                .child(opt_seg("cache-5g", "5 GB", 5120)),
                            p,
                        )
                        .into_any_element()
                    },
                ],
                p,
            )
            .into_any_element(),
        ]
    }
}
