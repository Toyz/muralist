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
use crate::wallhaven::{Tag, Wallpaper};

/// Background threads record too; writes go one at a time.
static WRITE: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    pub wallpaper: Wallpaper,
    /// When it was first saved (Unix seconds).
    pub saved: i64,
    /// Its tags, once looked up (search results don't carry them).
    #[serde(default)]
    pub tags: Option<Vec<Tag>>,
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

/// Notes that `w` was saved. Keeps the first save time, and any tags
/// already known.
pub fn remember(w: &Wallpaper) -> Result<()> {
    record(w, None)
}

/// Notes `w` along with its tags.
pub fn remember_with_tags(w: &Wallpaper, tags: Vec<Tag>) -> Result<()> {
    record(w, Some(tags))
}

/// Adds tags to a wallpaper already on record, if they're new.
pub fn set_tags(id: &str, tags: &[Tag]) -> Result<()> {
    let _guard = WRITE.lock().unwrap_or_else(|e| e.into_inner());
    let mut p = Provenance::load();
    match p.records.get_mut(id) {
        Some(r) if r.tags.as_deref() != Some(tags) => r.tags = Some(tags.to_vec()),
        _ => return Ok(()),
    }
    write(&p)
}

fn record(w: &Wallpaper, tags: Option<Vec<Tag>>) -> Result<()> {
    let _guard = WRITE.lock().unwrap_or_else(|e| e.into_inner());
    let mut p = Provenance::load();
    let r = p.records.entry(w.id.clone()).or_insert_with(|| Record {
        wallpaper: w.clone(),
        saved: chrono::Utc::now().timestamp(),
        tags: None,
    });
    r.wallpaper = w.clone();
    if tags.is_some() {
        r.tags = tags;
    }
    write(&p)
}

/// Writes to a temporary file, then renames it over the store, so a reader
/// never sees half of it.
fn write(p: &Provenance) -> Result<()> {
    let path = path();
    fs::create_dir_all(path.parent().unwrap())?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(p)?).with_context(|| format!("writing {}", tmp.display()))?;
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
