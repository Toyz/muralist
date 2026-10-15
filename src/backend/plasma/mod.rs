//! KDE Plasma (Wayland or X11): monitors from `kscreen-doctor`, desktop via
//! plasmashell scripting over D-Bus, lock screen via `kscreenlockerrc` plus a
//! small per-monitor wallpaper plugin.

mod kscreen;
mod shell;

use anyhow::Result;

use super::{Backend, Capabilities, Images, primary_image};
use crate::config::LockMode;
use crate::monitors::Monitor;

pub(super) struct Plasma;

impl Backend for Plasma {
    fn name(&self) -> &str {
        "KDE Plasma"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities { per_monitor_desktop: true, lock_screen: true, per_monitor_lock: true, read_current: true }
    }

    fn monitors(&self) -> Result<Vec<Monitor>> {
        kscreen::detect()
    }

    fn current_desktop(&self, monitors: &[Monitor]) -> Result<Images> {
        shell::current_desktop(monitors)
    }

    fn set_desktop(&self, images: &Images, monitors: &[Monitor]) -> Result<()> {
        shell::apply_desktop(images, monitors)
    }

    fn current_lock(&self, monitors: &[Monitor]) -> Result<Images> {
        shell::current_lock(monitors)
    }

    fn set_lock(&self, images: &Images, monitors: &[Monitor], mode: LockMode) -> Result<()> {
        match mode {
            LockMode::PerMonitor => shell::apply_lock_per_monitor(images, monitors),
            LockMode::Single => match primary_image(images, monitors) {
                Some(path) => shell::apply_lock_single(path),
                None => anyhow::bail!("nothing assigned to any monitor"),
            },
        }
    }
}
