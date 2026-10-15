//! Where each saved wallpaper came from: its Wallhaven record (purity,
//! category, resolution, colors), kept so the Library and History know what
//! a file on disk is long after it was downloaded. Stored in
//! `~/.config/muralist/provenance.json`, keyed by Wallhaven ID, which every
//! file muralist writes carries in its name (originals, cached originals and
//! span slices alike).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::config::config_dir;
use crate::wallhaven::{Tag, Wallpaper};

/// The store, read from disk once and kept: every thread records through
/// it, one at a time, and the UI merges the records it gets back instead
/// of re-reading the whole file after each write.
static STORE: Mutex<Option<Provenance>> = Mutex::new(None);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    pub wallpaper: Wallpaper,
    /// When it was first saved (Unix seconds).
    pub saved: i64,
    /// Its tags, once looked up (search results don't carry them).
    #[serde(default)]
    pub tags: Option<Vec<Tag>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Provenance {
    #[serde(default)]
    pub records: BTreeMap<String, Record>,
}

fn path() -> PathBuf {
    config_dir().join("provenance.json")
}

/// Runs `f` on the store, loading it on first use.
fn with_store<R>(f: impl FnOnce(&mut Provenance) -> R) -> R {
    let mut store = STORE.lock().unwrap_or_else(PoisonError::into_inner);
    f(store.get_or_insert_with(|| crate::store::load_json(&path())))
}

impl Provenance {
    /// A copy of everything on record.
    pub fn load() -> Self {
        with_store(|p| p.clone())
    }

    /// The record for a file muralist wrote, if there is one.
    pub fn of(&self, file: &Path) -> Option<&Record> {
        self.records.get(wallhaven_id(file)?)
    }
}

/// Notes that `w` was saved. Keeps the first save time, and any tags
/// already known. Returns the record as it now stands.
pub fn remember(w: &Wallpaper) -> Result<Record> {
    record(w, None)
}

/// Notes `w` along with its tags.
pub fn remember_with_tags(w: &Wallpaper, tags: Vec<Tag>) -> Result<Record> {
    record(w, Some(tags))
}

/// Adds tags to a wallpaper already on record, if they're new. Returns the
/// updated record, or `None` when nothing changed.
pub fn set_tags(id: &str, tags: &[Tag]) -> Result<Option<Record>> {
    with_store(|p| {
        match p.records.get_mut(id) {
            Some(r) if r.tags.as_deref() != Some(tags) => r.tags = Some(tags.to_vec()),
            _ => return Ok(None),
        }
        let r = p.records[id].clone();
        crate::store::save_json(&path(), p)?;
        Ok(Some(r))
    })
}

fn record(w: &Wallpaper, tags: Option<Vec<Tag>>) -> Result<Record> {
    with_store(|p| {
        let r = p.records.entry(w.id.clone()).or_insert_with(|| Record {
            wallpaper: w.clone(),
            saved: chrono::Utc::now().timestamp(),
            tags: None,
        });
        r.wallpaper = w.clone();
        if tags.is_some() {
            r.tags = tags;
        }
        let r = r.clone();
        crate::store::save_json(&path(), p)?;
        Ok(r)
    })
}

/// The Wallhaven ID in a file name: `wallhaven-abc123.jpg`,
/// `full-abc123.png` (cached originals), or a span slice cut from one,
/// `wallhaven-abc123-span-DP-1.jpg`.
pub fn wallhaven_id(file: &Path) -> Option<&str> {
    let stem = file.file_stem()?.to_str()?;
    let rest = stem.strip_prefix("wallhaven-").or_else(|| stem.strip_prefix("full-"))?;
    let end = rest.bytes().position(|b| !b.is_ascii_alphanumeric()).unwrap_or(rest.len());
    (end >= 4).then(|| &rest[..end])
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
