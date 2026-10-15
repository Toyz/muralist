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
    pub fn hides(self, purity: &str) -> bool {
        match self {
            Spoiler::Off => false,
            Spoiler::Nsfw => purity == "nsfw",
            Spoiler::Sketchy => purity == "nsfw" || purity == "sketchy",
        }
    }
}

/// A past search, with a few of its results for the preview strip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
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
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
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
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?
        } else {
            let cfg = Config::default();
            cfg.save()?;
            cfg
        };
        if !cfg.min_resolution.is_empty() {
            if cfg.filters.atleast.is_none() {
                cfg.filters.atleast = Some(std::mem::take(&mut cfg.min_resolution));
            }
            cfg.min_resolution.clear();
            cfg.save()?;
        }
        cfg.env_api_key = std::env::var("WALLHAVEN_API_KEY").ok().filter(|k| !k.trim().is_empty());
        Ok(cfg)
    }

    pub fn effective_api_key(&self) -> &str {
        self.env_api_key.as_deref().unwrap_or(&self.api_key)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path();
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(&path, toml::to_string_pretty(self)?).with_context(|| format!("writing {}", path.display()))
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// Unix seconds.
    pub at: i64,
    /// "desktop" or "lock".
    pub target: String,
    pub images: BTreeMap<String, PathBuf>,
}

/// Oldest entries are dropped past this.
const HISTORY_LIMIT: usize = 400;

impl State {
    pub fn record(&mut self, target: &str, images: &BTreeMap<String, PathBuf>) {
        let same_as_last = self.history.iter().rev().find(|e| e.target == target).is_some_and(|e| &e.images == images);
        if same_as_last || images.is_empty() {
            return;
        }
        self.history.push(HistoryEntry {
            at: chrono::Utc::now().timestamp(),
            target: target.to_owned(),
            images: images.clone(),
        });
        if self.history.len() > HISTORY_LIMIT {
            let excess = self.history.len() - HISTORY_LIMIT;
            self.history.drain(..excess);
        }
    }
}

impl State {
    fn path() -> PathBuf {
        config_dir().join("state.json")
    }

    pub fn load() -> Self {
        fs::read_to_string(Self::path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path();
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(&path, serde_json::to_string_pretty(self)?)?;
        Ok(())
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
