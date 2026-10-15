//! KDE Plasma (Wayland or X11): monitors from `kscreen-doctor`, desktop via
//! plasmashell scripting over D-Bus, lock screen via `kscreenlockerrc` plus a
//! small per-monitor wallpaper plugin.

mod kscreen;
mod shell;

use std::path::Path;

use anyhow::Result;

use super::{Backend, Caps, Desktop, Images, Lock, Ops, ReadBack, SetEach, SetOne};
use crate::monitors::Monitor;

pub struct Plasma;

impl Backend for Plasma {
    const ID: &'static str = "plasma";
    const NAME: &'static str = "KDE Plasma";
    const CAPS: Caps<Self> = Caps { desktop: Ops::each().read(), lock: Some(Ops::each().read()) };

    fn matches(desktop: &str) -> bool {
        desktop.split(':').any(|d| d.eq_ignore_ascii_case("KDE"))
    }

    fn connect() -> Result<Self> {
        Ok(Self)
    }

    fn monitors(&self) -> Result<Vec<Monitor>> {
        kscreen::detect()
    }
}

impl SetOne<Desktop> for Plasma {
    fn set_one(&self, image: &Path, monitors: &[Monitor]) -> Result<()> {
        let images = monitors.iter().map(|m| (m.name.clone(), image.to_path_buf())).collect();
        shell::apply_desktop(&images, monitors)
    }
}

impl SetEach<Desktop> for Plasma {
    fn set_each(&self, images: &Images, monitors: &[Monitor]) -> Result<()> {
        shell::apply_desktop(images, monitors)
    }
}

impl ReadBack<Desktop> for Plasma {
    fn current(&self, monitors: &[Monitor]) -> Result<Images> {
        shell::current_desktop(monitors)
    }
}

/// One image uses Plasma's stock image plugin; per monitor uses ours.
impl SetOne<Lock> for Plasma {
    fn set_one(&self, image: &Path, _monitors: &[Monitor]) -> Result<()> {
        shell::apply_lock_single(image)
    }
}

impl SetEach<Lock> for Plasma {
    fn set_each(&self, images: &Images, monitors: &[Monitor]) -> Result<()> {
        shell::apply_lock_per_monitor(images, monitors)
    }
}

impl ReadBack<Lock> for Plasma {
    fn current(&self, monitors: &[Monitor]) -> Result<Images> {
        shell::current_lock(monitors)
    }
}
