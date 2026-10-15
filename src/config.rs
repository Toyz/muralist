use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

/// Which cards hide behind a pixel mosaic until hovered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Spoiler {
    Off,
    #[default]
    Nsfw,
    /// NSFW and Sketchy.
    Sketchy,
}

impl Spoiler {
    pub fn hides(self, purity: crate::wallhaven::Purity) -> bool {
        use crate::wallhaven::Purity;
        match self {
            Spoiler::Off => false,
            Spoiler::Nsfw => purity == Purity::Nsfw,
            Spoiler::Sketchy => purity != Purity::Sfw,
        }
    }
}

/// A past search, with a few of its results for the preview strip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentQuery {
    pub text: String,
    /// (wallpaper id, small thumbnail url)
    #[serde(default)]
    pub thumbs: Vec<(String, String)>,
}

/// Where auto-rotate picks wallpapers from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum RotateSource {
    /// Random results for the current filters.
    #[default]
    Search,
    /// Wallpapers already downloaded.
    Library,
    /// One of the galleries, by ID.
    Gallery(u64),
}

/// The order a gallery plays in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum RotateOrder {
    #[default]
    Shuffle,
    InOrder,
}

/// How the results grid is laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum GridMode {
    /// Uniform cards.
    #[default]
    Grid,
    /// Justified rows that keep each wallpaper's real shape.
    Mosaic,
}

/// How the lock screen wallpaper is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum LockMode {
    /// Every monitor on the lock screen gets its own image, where the
    /// backend supports it.
    #[default]
    PerMonitor,
    /// One image on every monitor (the primary's).
    Single,
}

/// The config format this build writes. History: 0, unversioned (an old
/// `min_resolution` key may linger); 1, versioned, `min_resolution` folded
/// into the search filters.
pub const CONFIG_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// The config's format version. Its own default (0, not the struct's)
    /// so a file written before versioning reads as version 0.
    #[serde(default)]
    pub version: u32,
    /// Wallhaven API key. `WALLHAVEN_API_KEY` in the environment wins over this.
    pub api_key: String,
    /// Where full-size wallpapers are saved.
    pub download_dir: PathBuf,
    pub lock_mode: LockMode,
    /// Legacy minimum-resolution key; migrated into `filters.atleast` on load.
    #[serde(skip_serializing)]
    pub min_resolution: String,
    /// Filters you changed away from your Wallhaven account defaults.
    pub filters: FilterOverrides,
    /// Fetch originals the cursor is heading for before they're clicked.
    pub precog: bool,
    /// Tint the interface with the selected wallpaper's colors.
    pub adaptive_theme: bool,
    pub layout: GridMode,
    /// Order results by hue.
    pub rainbow: bool,
    /// Size limit for thumbnails and cached originals, in MB; 0 is none.
    pub cache_limit_mb: u32,
    /// Panel widths, as last dragged.
    pub sidebar_width: f32,
    pub inspector_width: f32,
    /// Auto-rotate interval in minutes; 0 is off.
    pub rotate_minutes: u32,
    pub rotate_source: RotateSource,
    /// Darker wallpapers at night, brighter by day.
    pub rotate_match_time: bool,
    /// Rotate the lock screen too.
    pub rotate_lock: bool,
    pub rotate_order: RotateOrder,
    /// Where in-order playback of a gallery is up to.
    pub rotate_cursor: usize,
    /// For importing collections; the API key alone doesn't reveal it.
    pub wallhaven_user: String,
    pub spoiler: Spoiler,
    /// Hovering a hidden card reveals it; off means only selecting does.
    pub spoiler_hover: bool,
    /// Newest first.
    pub recent: Vec<RecentQuery>,
    /// `WALLHAVEN_API_KEY` from the environment; wins over `api_key` but is
    /// never written to disk.
    #[serde(skip)]
    pub env_api_key: Option<String>,
}

/// Search filters that differ from the account defaults. Unset fields follow
/// the account; an empty string means "no filter" for the string fields.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FilterOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sorting: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_range: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ratios: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub atleast: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolutions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub categories: Option<[bool; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub purity: Option<[bool; 3]>,
}

impl FilterOverrides {
    pub fn count(&self) -> usize {
        [
            self.sorting.is_some(),
            self.top_range.is_some(),
            self.ratios.is_some(),
            self.atleast.is_some(),
            self.resolutions.is_some(),
            self.color.is_some(),
            self.categories.is_some(),
            self.purity.is_some(),
        ]
        .into_iter()
        .filter(|b| *b)
        .count()
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            api_key: String::new(),
            download_dir: dirs::picture_dir()
                .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join("Pictures"))
                .join("muralist"),
            lock_mode: LockMode::default(),
            min_resolution: String::new(),
            filters: FilterOverrides::default(),
            precog: true,
            adaptive_theme: true,
            layout: GridMode::default(),
            rainbow: false,
            cache_limit_mb: 1024,
            sidebar_width: 264.,
            inspector_width: 420.,
            rotate_minutes: 0,
            rotate_source: RotateSource::default(),
            rotate_match_time: true,
            rotate_lock: false,
            rotate_order: RotateOrder::default(),
            rotate_cursor: 0,
            wallhaven_user: String::new(),
            spoiler: Spoiler::default(),
            spoiler_hover: true,
            recent: Vec::new(),
            env_api_key: None,
        }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        config_dir().join("config.toml")
    }

    /// Load the config, writing a default one on first run.
    pub fn load() -> Result<Self> {
        let path = Self::path();
        let mut cfg = if path.exists() {
            let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
            match toml::from_str(&text) {
                Ok(cfg) => cfg,
                // A torn or hand-broken config shouldn't stop the app: keep
                // it aside for the user and start from defaults.
                Err(e) => {
                    let aside = path.with_extension("toml.bad");
                    eprintln!("muralist: {} is unreadable ({e}); kept it as {}", path.display(), aside.display());
                    let _ = fs::rename(&path, &aside);
                    let cfg = Config::default();
                    cfg.save()?;
                    cfg
                }
            }
        } else {
            let cfg = Config::default();
            cfg.save()?;
            cfg
        };
        if cfg.version > CONFIG_VERSION {
            eprintln!(
                "muralist: {} was written by a newer muralist (format {}, this one knows {CONFIG_VERSION}); kept a copy",
                path.display(),
                cfg.version
            );
            crate::store::keep_copy(&path, cfg.version);
        } else if cfg.version < CONFIG_VERSION {
            crate::store::keep_copy(&path, cfg.version);
            cfg.upgrade();
            cfg.save()?;
        }
        cfg.env_api_key = std::env::var("WALLHAVEN_API_KEY").ok().filter(|k| !k.trim().is_empty());
        Ok(cfg)
    }

    /// Brings a config read from an older format up to this one, a version
    /// at a time.
    fn upgrade(&mut self) {
        for from in self.version..CONFIG_VERSION {
            if from == 0 && !self.min_resolution.is_empty() {
                if self.filters.atleast.is_none() {
                    self.filters.atleast = Some(std::mem::take(&mut self.min_resolution));
                }
                self.min_resolution.clear();
            }
        }
        self.version = CONFIG_VERSION;
    }

    pub fn effective_api_key(&self) -> &str {
        self.env_api_key.as_deref().unwrap_or(&self.api_key)
    }

    pub fn save(&self) -> Result<()> {
        crate::store::write_private(&Self::path(), toml::to_string_pretty(self)?.as_bytes())
    }
}

/// Last applied assignments, keyed by output name (e.g. "DP-1").
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub desktop: BTreeMap<String, PathBuf>,
    pub lock: BTreeMap<String, PathBuf>,
    /// Every set of wallpapers applied, oldest first.
    pub history: Vec<HistoryEntry>,
}

/// Which wallpaper a history entry set. Stored as "desktop" or "lock".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Surface {
    Desktop,
    Lock,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// Unix seconds.
    pub at: i64,
    pub target: Surface,
    pub images: BTreeMap<String, PathBuf>,
}

/// Oldest entries are dropped past this.
const HISTORY_LIMIT: usize = 400;

impl State {
    pub fn record(&mut self, target: Surface, images: &BTreeMap<String, PathBuf>) {
        let same_as_last = self.history.iter().rev().find(|e| e.target == target).is_some_and(|e| &e.images == images);
        if same_as_last || images.is_empty() {
            return;
        }
        self.history.push(HistoryEntry { at: chrono::Utc::now().timestamp(), target, images: images.clone() });
        if self.history.len() > HISTORY_LIMIT {
            let excess = self.history.len() - HISTORY_LIMIT;
            self.history.drain(..excess);
        }
    }
}

/// Format history: 0, unversioned; 1, versioned, history targets typed
/// (same words on disk).
impl crate::store::Versioned for State {
    const VERSION: u32 = 1;
}

impl State {
    fn path() -> PathBuf {
        config_dir().join("state.json")
    }

    pub fn load() -> Self {
        crate::store::load_versioned(&Self::path())
    }

    pub fn save(&self) -> Result<()> {
        crate::store::save_versioned(&Self::path(), self)
    }
}

/// Before the rename the app kept its files under `wally/`. Move them over
/// once, leaving a symlink behind: lock-screen settings and history hold
/// absolute paths into the old cache dir.
pub fn migrate_legacy_dirs() {
    for base in [dirs::config_dir(), dirs::cache_dir()].into_iter().flatten() {
        let (old, new) = (base.join("wally"), base.join("muralist"));
        if new.exists() || !old.is_dir() || old.is_symlink() {
            continue;
        }
        if fs::rename(&old, &new).is_ok() {
            #[cfg(unix)]
            let _ = std::os::unix::fs::symlink(&new, &old);
        }
    }
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_default().join("muralist")
}

pub fn cache_dir() -> PathBuf {
    dirs::cache_dir().unwrap_or_default().join("muralist")
}

#[cfg(test)]
mod version_tests {
    use super::{CONFIG_VERSION, Config, HistoryEntry, Surface};

    #[test]
    fn an_unversioned_config_reads_as_version_zero_and_upgrades() {
        let mut cfg: Config = toml::from_str("min_resolution = \"1920x1080\"\n").unwrap();
        assert_eq!(cfg.version, 0);
        cfg.upgrade();
        assert_eq!(cfg.version, CONFIG_VERSION);
        assert_eq!(cfg.filters.atleast.as_deref(), Some("1920x1080"));
        assert!(cfg.min_resolution.is_empty());
        assert_eq!(Config::default().version, CONFIG_VERSION);
    }

    #[test]
    fn history_targets_keep_their_words_on_disk() {
        let e: HistoryEntry = serde_json::from_str(r#"{"at":1,"target":"lock","images":{}}"#).unwrap();
        assert_eq!(e.target, Surface::Lock);
        assert!(serde_json::to_string(&e).unwrap().contains(r#""target":"lock""#));
    }
}

/// Loads each real store under `$XDG_CONFIG_HOME/muralist`, saves it back
/// through the typed structs, and checks nothing but the version stamp
/// changed. Point it at a copy: `XDG_CONFIG_HOME=/tmp/copy cargo test
/// stores_round_trip -- --ignored`.
#[cfg(test)]
mod round_trip {
    use std::path::Path;

    fn raw(path: &Path) -> serde_json::Value {
        let mut v: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        v.as_object_mut().unwrap().remove("version");
        v
    }

    #[test]
    #[ignore = "reads and rewrites the stores under XDG_CONFIG_HOME; point it at a copy"]
    fn stores_round_trip() {
        let dir = super::config_dir();
        let galleries = dir.join("galleries.json");
        let before = raw(&galleries);
        crate::gallery::Galleries::load().save().unwrap();
        assert_eq!(raw(&galleries), before, "galleries changed");

        let state = dir.join("state.json");
        let before = raw(&state);
        super::State::load().save().unwrap();
        assert_eq!(raw(&state), before, "state changed");

        let provenance = dir.join("provenance.json");
        let before = raw(&provenance);
        let p = crate::provenance::Provenance::load();
        crate::store::save_versioned(&provenance, &p).unwrap();
        assert_eq!(raw(&provenance), before, "provenance changed");
    }
}
