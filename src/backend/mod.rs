//! Desktop backends: everything that talks to a particular desktop
//! environment or OS lives behind [`Backend`], so supporting a new one means
//! adding one module here and a line in [`detect`].
//!
//! Each backend says what it can do via [`Capabilities`]; the UI hides
//! controls a backend can't honor.
//!
//! Adding a desktop:
//! 1. `backend/<name>/mod.rs` with a type implementing [`Backend`]. Only
//!    `name`, `capabilities`, `monitors` and `set_desktop` are required; the
//!    rest default to "unsupported".
//! 2. Gate it with `#[cfg(...)]` if it's OS-specific, and put any
//!    OS-specific crates under `[target.'cfg(...)'.dependencies]`.
//! 3. Return it from [`detect`] when the session matches.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Result, bail};

use crate::config::LockMode;
use crate::monitors::Monitor;

#[cfg(target_os = "linux")]
mod plasma;
mod unsupported;

/// Image per output name.
pub type Images = BTreeMap<String, PathBuf>;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Capabilities {
    /// A different desktop image on each monitor.
    pub per_monitor_desktop: bool,
    /// Can set the lock screen at all.
    pub lock_screen: bool,
    /// A different lock-screen image on each monitor.
    pub per_monitor_lock: bool,
    /// Can report what is currently set (Peek restores from this).
    pub read_current: bool,
}

pub trait Backend: Send + Sync {
    /// Human name, e.g. "KDE Plasma".
    fn name(&self) -> &str;

    fn capabilities(&self) -> Capabilities;

    /// Whether this backend can do anything at all on this system.
    fn is_supported(&self) -> bool {
        true
    }

    /// Enabled displays, sorted left to right.
    fn monitors(&self) -> Result<Vec<Monitor>>;

    /// What each monitor's desktop shows now.
    fn current_desktop(&self, _monitors: &[Monitor]) -> Result<Images> {
        Ok(Images::new())
    }

    /// Set the desktop; monitors missing from `images` are left alone.
    fn set_desktop(&self, images: &Images, monitors: &[Monitor]) -> Result<()>;

    /// What each monitor's lock screen shows now.
    fn current_lock(&self, _monitors: &[Monitor]) -> Result<Images> {
        Ok(Images::new())
    }

    /// Set the lock screen. `mode` is only a preference: backends without
    /// per-monitor lock screens use [`primary_image`].
    fn set_lock(&self, _images: &Images, _monitors: &[Monitor], _mode: LockMode) -> Result<()> {
        bail!("{} can't set the lock screen", self.name())
    }
}

pub type SharedBackend = Arc<dyn Backend>;

/// Pick the backend for this session. `MURALIST_BACKEND` forces one.
pub fn detect() -> SharedBackend {
    let forced = std::env::var("MURALIST_BACKEND").ok().map(|b| b.to_ascii_lowercase());
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();

    #[cfg(target_os = "linux")]
    {
        let is_kde = desktop.split(':').any(|d| d.eq_ignore_ascii_case("KDE"));
        match forced.as_deref() {
            Some("plasma" | "kde") => return Arc::new(plasma::Plasma),
            None if is_kde => return Arc::new(plasma::Plasma),
            _ => {}
        }
    }
    let _ = forced;
    Arc::new(unsupported::Unsupported::new(desktop))
}

#[cfg(test)]
mod tests {
    use super::{Images, primary_image};
    use crate::monitors::Monitor;
    use std::path::PathBuf;

    fn mon(name: &str, x: i32, primary: bool) -> Monitor {
        Monitor { name: name.into(), x, y: 0, width: 1920, height: 1080, px_width: 1920, px_height: 1080, primary }
    }

    #[test]
    fn primary_image_prefers_the_primary_then_leftmost() {
        let ms = [mon("A", 0, false), mon("B", 1920, true)];
        let mut images = Images::new();
        images.insert("A".into(), PathBuf::from("a.jpg"));
        assert_eq!(primary_image(&images, &ms), Some(&PathBuf::from("a.jpg")));
        images.insert("B".into(), PathBuf::from("b.jpg"));
        assert_eq!(primary_image(&images, &ms), Some(&PathBuf::from("b.jpg")));
    }
}

/// The image for the primary monitor (or the leftmost), for backends that
/// can only show one image somewhere.
pub fn primary_image<'a>(images: &'a Images, monitors: &[Monitor]) -> Option<&'a PathBuf> {
    monitors
        .iter()
        .filter(|m| m.primary)
        .chain(monitors.iter())
        .find_map(|m| images.get(&m.name))
        .or_else(|| images.values().next())
}
