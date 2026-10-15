//! Galleries: named sets of wallpapers. A gallery is hand-picked, a saved
//! search that refreshes itself, or a copy of a Wallhaven collection. Kept in
//! `~/.config/muralist/galleries.json`, written atomically so a background
//! service can read it while the app runs.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

use crate::config::config_dir;
use crate::wallhaven::{Client, SearchParams, Sorting, Wallpaper};

/// Smart galleries keep this many results unless told otherwise.
pub const SMART_LIMIT: u32 = 48;
/// Collections are imported up to this many pages (24 per page).
const COLLECTION_PAGES: u32 = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Entry {
    Wallhaven(Wallpaper),
    Local(PathBuf),
}

impl Entry {
    /// Identity for de-duplication: a Wallhaven ID or a file path.
    pub fn key(&self) -> String {
        match self {
            Entry::Wallhaven(w) => format!("wh-{}", w.id),
            Entry::Local(p) => p.to_string_lossy().into_owned(),
        }
    }

    /// Where the full-size original lives once downloaded.
    pub fn original(&self, download_dir: &Path) -> PathBuf {
        match self {
            Entry::Wallhaven(w) => download_dir.join(w.file_name()),
            Entry::Local(p) => p.clone(),
        }
    }
}

/// A search, frozen so it can be run again later.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedSearch {
    pub query: String,
    pub categories: [bool; 3],
    pub purity: [bool; 3],
    pub sorting: String,
    pub top_range: String,
    #[serde(default)]
    pub ratios: Option<String>,
    #[serde(default)]
    pub atleast: Option<String>,
    #[serde(default)]
    pub resolutions: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
}

impl SavedSearch {
    pub fn from_params(p: &SearchParams) -> Self {
        Self {
            query: p.query.clone(),
            categories: p.categories,
            purity: p.purity,
            sorting: p.sorting.api().to_owned(),
            top_range: p.top_range.clone(),
            ratios: p.ratios.clone(),
            atleast: p.atleast.clone(),
            resolutions: p.resolutions.clone(),
            color: p.color.clone(),
        }
    }

    pub fn params(&self, page: u32) -> SearchParams {
        SearchParams {
            query: self.query.clone(),
            categories: self.categories,
            purity: self.purity,
            sorting: Sorting::from_api(&self.sorting).unwrap_or(Sorting::DateAdded),
            top_range: self.top_range.clone(),
            ratios: self.ratios.clone(),
            atleast: self.atleast.clone(),
            resolutions: self.resolutions.clone(),
            color: self.color.clone(),
            page,
            seed: None,
        }
    }

    /// Short human summary, e.g. `"cyberpunk" · Top 1M · 16:9`.
    pub fn describe(&self) -> String {
        let sorting = Sorting::from_api(&self.sorting).unwrap_or(Sorting::DateAdded);
        let mut parts = Vec::new();
        if !self.query.trim().is_empty() {
            parts.push(format!("\"{}\"", self.query.trim()));
        }
        parts.push(if sorting == Sorting::Toplist {
            format!("Top {}", self.top_range)
        } else {
            sorting.label().to_owned()
        });
        if let Some(c) = &self.color {
            parts.push(format!("#{c}"));
        }
        if let Some(r) = &self.atleast {
            parts.push(format!("{r}+"));
        }
        parts.join("  ·  ")
    }

    /// A gallery name for this search.
    pub fn suggested_name(&self) -> String {
        let q = self.query.trim();
        if q.is_empty() { self.describe() } else { q.to_owned() }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Source {
    /// Hand-picked.
    #[default]
    Manual,
    /// The top results of a saved search, refreshed on demand.
    Smart { search: SavedSearch, limit: u32 },
    /// A copy of a Wallhaven collection.
    Wallhaven { user: String, collection: u64 },
}

impl Source {
    /// Whether the items come from somewhere that can be fetched again.
    pub fn refreshable(&self) -> bool {
        !matches!(self, Source::Manual)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gallery {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub source: Source,
    #[serde(default)]
    pub items: Vec<Entry>,
    /// Keep every original downloaded, so rotation works offline.
    #[serde(default)]
    pub offline: bool,
    /// When the items last came from the source (Unix seconds); 0 = never.
    #[serde(default)]
    pub refreshed: i64,
    pub created: i64,
}

impl Gallery {
    pub fn contains(&self, key: &str) -> bool {
        self.items.iter().any(|e| e.key() == key)
    }

    /// Entries whose originals aren't downloaded yet.
    pub fn missing_originals<'a>(&'a self, download_dir: &'a Path) -> impl Iterator<Item = &'a Entry> + 'a {
        self.items.iter().filter(move |e| matches!(e, Entry::Wallhaven(_)) && !e.original(download_dir).is_file())
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Galleries {
    #[serde(default)]
    pub list: Vec<Gallery>,
}

impl Galleries {
    fn path() -> PathBuf {
        config_dir().join("galleries.json")
    }

    pub fn load() -> Self {
        fs::read_to_string(Self::path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    /// Write to a temporary file, then rename over the old one, so a reader
    /// never sees half a file.
    pub fn save(&self) -> Result<()> {
        let path = Self::path();
        fs::create_dir_all(path.parent().unwrap())?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_string_pretty(self)?).with_context(|| format!("writing {}", tmp.display()))?;
        fs::rename(&tmp, &path).with_context(|| format!("replacing {}", path.display()))
    }

    pub fn get(&self, id: u64) -> Option<&Gallery> {
        self.list.iter().find(|g| g.id == id)
    }

    pub fn get_mut(&mut self, id: u64) -> Option<&mut Gallery> {
        self.list.iter_mut().find(|g| g.id == id)
    }

    /// Adds a gallery and returns its ID.
    pub fn create(&mut self, name: impl Into<String>, source: Source) -> u64 {
        let now = chrono::Utc::now();
        // Milliseconds, nudged past any existing ID so two in a row differ.
        let mut id = now.timestamp_millis().max(0) as u64;
        while self.get(id).is_some() {
            id += 1;
        }
        self.list.push(Gallery {
            id,
            name: name.into(),
            source,
            items: Vec::new(),
            offline: false,
            refreshed: 0,
            created: now.timestamp(),
        });
        id
    }

    pub fn remove(&mut self, id: u64) {
        self.list.retain(|g| g.id != id);
    }

    /// A name not already taken: "Gallery", "Gallery 2", ...
    pub fn unique_name(&self, base: &str) -> String {
        let taken = |n: &str| self.list.iter().any(|g| g.name.eq_ignore_ascii_case(n));
        if !taken(base) {
            return base.to_owned();
        }
        (2..).map(|i| format!("{base} {i}")).find(|n| !taken(n)).unwrap()
    }

    /// Adds `entry` unless it's already there. Returns whether it was added.
    pub fn add(&mut self, id: u64, entry: Entry) -> bool {
        let Some(g) = self.get_mut(id) else { return false };
        if g.contains(&entry.key()) {
            return false;
        }
        g.items.push(entry);
        true
    }

    /// Adds `entry` if missing, removes it if present. Returns whether it is
    /// in the gallery afterwards.
    pub fn toggle(&mut self, id: u64, entry: Entry) -> bool {
        let Some(g) = self.get_mut(id) else { return false };
        let key = entry.key();
        if g.contains(&key) {
            g.items.retain(|e| e.key() != key);
            false
        } else {
            g.items.push(entry);
            true
        }
    }
}

/// Fetches a refreshable source's current items. Blocking; run it off the
/// UI thread.
pub fn fetch_source(client: &Client, source: &Source, purity: [bool; 3]) -> Result<Vec<Entry>> {
    let mut out: Vec<Wallpaper> = Vec::new();
    match source {
        Source::Manual => {}
        Source::Smart { search, limit } => {
            let mut page = 1;
            while out.len() < *limit as usize {
                let resp = client.search(&search.params(page))?;
                let last = resp.meta.last_page;
                out.extend(resp.data);
                if page >= last {
                    break;
                }
                page += 1;
            }
            out.truncate(*limit as usize);
        }
        Source::Wallhaven { user, collection } => {
            for page in 1..=COLLECTION_PAGES {
                let resp = client.collection(user, *collection, purity, page)?;
                let last = resp.meta.last_page;
                out.extend(resp.data);
                if page >= last {
                    break;
                }
            }
        }
    }
    Ok(out.into_iter().map(Entry::Wallhaven).collect())
}

#[cfg(test)]
mod tests {
    use super::{Entry, Galleries, SavedSearch, Source};
    use crate::wallhaven::{SearchParams, Sorting};
    use std::path::PathBuf;

    fn local(name: &str) -> Entry {
        Entry::Local(PathBuf::from(name))
    }

    #[test]
    fn add_and_toggle_keep_entries_unique() {
        let mut g = Galleries::default();
        let id = g.create("Night", Source::Manual);
        assert!(g.add(id, local("/a.jpg")));
        assert!(!g.add(id, local("/a.jpg")));
        assert!(g.toggle(id, local("/b.jpg")));
        assert!(!g.toggle(id, local("/a.jpg")));
        let keys: Vec<String> = g.get(id).unwrap().items.iter().map(Entry::key).collect();
        assert_eq!(keys, ["/b.jpg"]);
    }

    #[test]
    fn ids_and_names_stay_distinct() {
        let mut g = Galleries::default();
        let a = g.create(g.unique_name("Gallery"), Source::Manual);
        let b = g.create(g.unique_name("Gallery"), Source::Manual);
        assert_ne!(a, b);
        assert_eq!(g.get(b).unwrap().name, "Gallery 2");
    }

    #[test]
    fn saved_search_round_trips_through_json() {
        let params = SearchParams {
            query: "neon city".into(),
            categories: [true, true, false],
            purity: [true, false, false],
            sorting: Sorting::Toplist,
            top_range: "1M".into(),
            ratios: Some("16x9".into()),
            atleast: Some("2560x1440".into()),
            resolutions: None,
            color: None,
            page: 3,
            seed: None,
        };
        let saved = SavedSearch::from_params(&params);
        let mut g = Galleries::default();
        let id = g.create(saved.suggested_name(), Source::Smart { search: saved.clone(), limit: 48 });
        let json = serde_json::to_string(&g).unwrap();
        let back: Galleries = serde_json::from_str(&json).unwrap();
        let gallery = back.get(id).unwrap();
        assert_eq!(gallery.name, "neon city");
        assert_eq!(gallery.source, Source::Smart { search: saved.clone(), limit: 48 });
        let again = saved.params(1);
        assert_eq!((again.sorting, again.page, again.atleast.as_deref()), (Sorting::Toplist, 1, Some("2560x1440")));
        assert_eq!(saved.describe(), "\"neon city\"  ·  Top 1M  ·  2560x1440+");
    }
}
