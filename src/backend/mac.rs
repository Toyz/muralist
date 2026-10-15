//! macOS: desktop pictures per screen through `NSWorkspace`. There is no
//! public lock screen API; macOS shows the desktop picture there anyway.
//!
//! macOS only changes the picture on each screen's current Space.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use dispatch2::run_on_main;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSScreen, NSWorkspace};
use objc2_foundation::{NSDictionary, NSString, NSURL};

use super::{Backend, Caps, Desktop, Images, Ops, ReadBack, SetEach, SetOne};
use crate::monitors::Monitor;

pub struct MacOs;

impl Backend for MacOs {
    const ID: &'static str = "macos";
    const NAME: &'static str = "macOS";
    const CAPS: Caps<Self> = Caps { desktop: Ops::each().read(), lock: None };

    fn matches(_desktop: &str) -> bool {
        true
    }

    fn connect() -> Result<Self> {
        Ok(Self)
    }

    fn monitors(&self) -> Result<Vec<Monitor>> {
        // AppKit wants the main thread; backend calls come from workers.
        Ok(run_on_main(|mtm| screens(mtm).into_iter().map(|(_, m)| m).collect()))
    }
}

impl SetOne<Desktop> for MacOs {
    fn set_one(&self, image: &Path, monitors: &[Monitor]) -> Result<()> {
        let images = monitors.iter().map(|m| (m.name.clone(), image.to_path_buf())).collect();
        self.set_each(&images, monitors)
    }
}

impl SetEach<Desktop> for MacOs {
    fn set_each(&self, images: &Images, _monitors: &[Monitor]) -> Result<()> {
        run_on_main(|mtm| {
            let workspace = NSWorkspace::sharedWorkspace();
            let options = NSDictionary::new();
            for (screen, monitor) in screens(mtm) {
                let Some(path) = images.get(&monitor.name) else { continue };
                let path = std::path::absolute(path)?;
                let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
                unsafe { workspace.setDesktopImageURL_forScreen_options_error(&url, &screen, &options) }.map_err(
                    |e| anyhow!("setting the desktop picture on {}: {}", monitor.name, e.localizedDescription()),
                )?;
            }
            Ok(())
        })
    }
}

impl ReadBack<Desktop> for MacOs {
    fn current(&self, _monitors: &[Monitor]) -> Result<Images> {
        Ok(run_on_main(|mtm| {
            let workspace = NSWorkspace::sharedWorkspace();
            screens(mtm)
                .into_iter()
                .filter_map(|(screen, monitor)| {
                    let path = workspace.desktopImageURLForScreen(&screen)?.path()?;
                    Some((monitor.name, PathBuf::from(path.to_string())))
                })
                .collect()
        }))
    }
}

/// Screens with their layout, left to right. AppKit's origin is the bottom
/// left of the menu-bar screen, so y is flipped to match other backends.
fn screens(mtm: MainThreadMarker) -> Vec<(Retained<NSScreen>, Monitor)> {
    let screens = NSScreen::screens(mtm);
    let menu_bar_h = screens.firstObject().map_or(0., |s| s.frame().size.height);
    let mut seen: BTreeMap<String, u32> = BTreeMap::new();
    let mut out: Vec<_> = screens
        .iter()
        .enumerate()
        .map(|(i, screen)| {
            let frame = screen.frame();
            let scale = screen.backingScaleFactor();
            // Two identical monitors share a name; number the repeats.
            let base = screen.localizedName().to_string();
            let n = seen.entry(base.clone()).and_modify(|n| *n += 1).or_insert(1);
            let name = if *n == 1 { base } else { format!("{base} ({n})") };
            let monitor = Monitor {
                name,
                x: frame.origin.x.round() as i32,
                y: (menu_bar_h - frame.origin.y - frame.size.height).round() as i32,
                width: frame.size.width.round() as u32,
                height: frame.size.height.round() as u32,
                px_width: (frame.size.width * scale).round() as u32,
                px_height: (frame.size.height * scale).round() as u32,
                primary: i == 0,
            };
            (screen, monitor)
        })
        .collect();
    out.sort_by_key(|(_, m)| (m.x, m.y));
    out
}
