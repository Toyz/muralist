//! Where each saved wallpaper came from: its Wallhaven record (purity,
//! category, resolution, colors), kept so the Library and History know what
//! a file on disk is long after it was downloaded. Stored in
//! `~/.config/muralist/provenance.json`, keyed by Wallhaven ID, which every
//! file muralist writes carries in its name (originals, cached originals and
//! span slices alike).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

use crate::config::config_dir;
use crate::wallhaven::Wallpaper;

/// Background threads record too; writes go one at a time.
static WRITE: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    pub wallpaper: Wallpaper,
    /// When it was first saved (Unix seconds).
    pub saved: i64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Provenance {
    #[serde(default)]
    pub records: BTreeMap<String, Record>,
}

fn path() -> PathBuf {
    config_dir().join("provenance.json")
}

impl Provenance {
    pub fn load() -> Self {
        fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    /// The record for a file muralist wrote, if there is one.
    pub fn of(&self, file: &Path) -> Option<&Record> {
        self.records.get(&wallhaven_id(file)?)
    }
}

/// Notes that `w` was saved. Keeps the first save time if it's already known.
pub fn remember(w: &Wallpaper) -> Result<()> {
    let _guard = WRITE.lock().unwrap_or_else(|e| e.into_inner());
    let mut p = Provenance::load();
    let saved = p.records.get(&w.id).map_or_else(|| chrono::Utc::now().timestamp(), |r| r.saved);
    p.records.insert(w.id.clone(), Record { wallpaper: w.clone(), saved });
    let path = path();
    fs::create_dir_all(path.parent().unwrap())?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(&p)?).with_context(|| format!("writing {}", tmp.display()))?;
    fs::rename(&tmp, &path).with_context(|| format!("replacing {}", path.display()))
}

/// The Wallhaven ID in a file name: `wallhaven-abc123.jpg`,
/// `full-abc123.png` (cached originals), or a span slice cut from one,
/// `wallhaven-abc123-span-DP-1.jpg`.
pub fn wallhaven_id(file: &Path) -> Option<String> {
    let stem = file.file_stem()?.to_str()?;
    let rest = stem.strip_prefix("wallhaven-").or_else(|| stem.strip_prefix("full-"))?;
    let id: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
    (id.len() >= 4).then_some(id)
}

#[cfg(test)]
mod tests {
    use super::wallhaven_id;
    use std::path::Path;

    #[test]
    fn ids_come_from_every_kind_of_file_name() {
        assert_eq!(wallhaven_id(Path::new("/p/wallhaven-9o91kk.png")).as_deref(), Some("9o91kk"));
        assert_eq!(wallhaven_id(Path::new("/c/full-og78el.jpg")).as_deref(), Some("og78el"));
        assert_eq!(wallhaven_id(Path::new("/s/wallhaven-k8j6l6-span-z120x10y0-DP-1.jpg")).as_deref(), Some("k8j6l6"));
        assert_eq!(wallhaven_id(Path::new("/p/holiday.jpg")), None);
        assert_eq!(wallhaven_id(Path::new("/p/wallhaven-.jpg")), None);
    }
}
