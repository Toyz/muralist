mod backend;
mod config;
mod gallery;
mod monitors;
mod span;
mod thumbs;
mod ui;
mod wallhaven;

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::*;

fn main() {
    config::migrate_legacy_dirs();
    let cfg = match config::Config::load() {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("muralist: {e:#}");
            std::process::exit(1);
        }
    };

    gpui_kit::application().with_assets(gpui_kit::assets::Assets).run(move |cx| {
        gpui_kit::init(cx);
        // One window: closing it quits, macOS included.
        cx.set_quit_mode(QuitMode::LastWindowClosed);
        Theme::change(ThemeMode::Dark, None, cx);
        ui::apply_theme(cx);

        let bounds = Bounds::centered(None, size(px(1640.), px(1000.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("muralist".into()),
                appears_transparent: true,
                // Centered in the sidebar's title bar.
                traffic_light_position: Some(point(px(18.), px(21.))),
            }),
            window_decorations: Some(WindowDecorations::Client),
            // Opaque: the compositor then ignores alpha inside the frame, so
            // translucent layers can't punch holes through to the desktop.
            // The shadow outside the frame still blends.
            window_background: WindowBackgroundAppearance::Opaque,
            app_id: Some("muralist".into()),
            window_min_size: Some(size(px(1180.), px(720.))),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| ui::Muralist::new(cfg, window, cx)))
            .expect("failed to open window");
    });
}
