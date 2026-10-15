//! The files muralist keeps for itself (config, state, galleries,
//! provenance) and the ones it downloads or renders: always written whole
//! or not at all, under a temporary name nobody else is using, and read
//! back forgivingly without ever quietly throwing the user's data away.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context as _, Result};
use serde::Serialize;
use serde::de::DeserializeOwned;

static SEQ: AtomicU64 = AtomicU64::new(0);

/// A temporary sibling of `dest` that no other writer (another thread, a
/// cancelled download being restarted) can be using. Removed when dropped
/// unless it was persisted.
pub struct TempFile {
    path: PathBuf,
    armed: bool,
}

impl TempFile {
    pub fn beside(dest: &Path, ext: &str) -> Self {
        let name = dest.file_name().map_or_else(|| "file".into(), |n| n.to_string_lossy().into_owned());
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let path = dest.with_file_name(format!(".{name}.{}-{seq}.{ext}", std::process::id()));
        Self { path, armed: true }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Moves it over `dest`; from here it's no longer cleaned up.
    pub fn persist(mut self, dest: &Path) -> Result<()> {
        fs::rename(&self.path, dest).with_context(|| format!("replacing {}", dest.display()))?;
        self.armed = false;
        Ok(())
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// Writes `bytes` to `dest` atomically: a reader sees the old file or the
/// new one, never half of either, and a crash leaves no torn file behind.
pub fn write_atomic(dest: &Path, bytes: &[u8]) -> Result<()> {
    write_atomic_with(dest, bytes, None)
}

/// [`write_atomic`] for files only the user should read (the config holds
/// the API key).
pub fn write_private(dest: &Path, bytes: &[u8]) -> Result<()> {
    write_atomic_with(dest, bytes, Some(0o600))
}

fn write_atomic_with(dest: &Path, bytes: &[u8], mode: Option<u32>) -> Result<()> {
    use std::io::Write as _;
    if let Some(dir) = dest.parent() {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let tmp = TempFile::beside(dest, "tmp");
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if let Some(mode) = mode {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(mode);
    }
    #[cfg(not(unix))]
    let _ = mode;
    let mut file = options.open(tmp.path()).with_context(|| format!("writing {}", tmp.path().display()))?;
    file.write_all(bytes).with_context(|| format!("writing {}", tmp.path().display()))?;
    file.sync_all().ok();
    drop(file);
    tmp.persist(dest)
}

/// A store whose file carries a format version, so a later muralist knows
/// what shape it is in instead of guessing.
pub trait Versioned: Serialize + DeserializeOwned + Default {
    /// The version this build writes.
    const VERSION: u32;

    /// Upgrades the file's raw JSON from version `from` to `from + 1`.
    /// Files written before versioning count as version 0.
    fn upgrade(json: &mut serde_json::Value, from: u32) -> Result<()> {
        let _ = (json, from);
        Ok(())
    }
}

/// The version stamped in a JSON store's raw text (0 if none).
fn version_of(json: &serde_json::Value) -> u32 {
    json.get("version").and_then(serde_json::Value::as_u64).map_or(0, |v| u32::try_from(v).unwrap_or(u32::MAX))
}

/// Keeps a copy of `path` as `<name>.v<version>.bak`, once.
pub fn keep_copy(path: &Path, version: u32) {
    let copy = path.with_extension(format!(
        "{}.v{version}.bak",
        path.extension().map_or_else(String::new, |e| e.to_string_lossy().into_owned())
    ));
    if !copy.exists() {
        let _ = fs::copy(path, &copy);
    }
}

/// Reads a versioned JSON store, upgrading an older file one version at a
/// time (after keeping a copy of it as it was). A file from a newer muralist
/// is read as best this build can, and kept aside before anything is ever
/// written over it. One that won't read at all is set aside as corrupt.
pub fn load_versioned<T: Versioned>(path: &Path) -> T {
    let Ok(text) = fs::read_to_string(path) else { return T::default() };
    let set_aside = |why: String| {
        let aside = path.with_extension(format!("corrupt-{}", chrono::Utc::now().timestamp()));
        eprintln!("muralist: {} is unreadable ({why}); kept it as {}", path.display(), aside.display());
        let _ = fs::rename(path, &aside);
        T::default()
    };
    let mut json: serde_json::Value = match serde_json::from_str(&text) {
        Ok(json) => json,
        Err(e) => return set_aside(e.to_string()),
    };
    let found = version_of(&json);
    if found > T::VERSION {
        eprintln!(
            "muralist: {} was written by a newer muralist (format {found}, this one knows {}); kept a copy",
            path.display(),
            T::VERSION
        );
        keep_copy(path, found);
    } else if found < T::VERSION {
        keep_copy(path, found);
        for from in found..T::VERSION {
            if let Err(e) = T::upgrade(&mut json, from) {
                return set_aside(format!("upgrading from format {from}: {e:#}"));
            }
        }
    }
    match serde_json::from_value(json) {
        Ok(v) => v,
        Err(e) => set_aside(e.to_string()),
    }
}

/// Saves a versioned JSON store atomically, stamped with this build's version.
pub fn save_versioned<T: Versioned>(path: &Path, value: &T) -> Result<()> {
    let mut json = serde_json::to_value(value)?;
    if let Some(map) = json.as_object_mut() {
        map.insert("version".into(), T::VERSION.into());
    }
    write_atomic(path, &serde_json::to_vec(&json)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("muralist-store-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn atomic_write_replaces_and_leaves_no_temp() {
        let dir = scratch("atomic");
        let dest = dir.join("a.json");
        write_atomic(&dest, b"one").unwrap();
        write_atomic(&dest, b"two").unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"two");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
    }

    #[test]
    fn temp_names_never_collide_and_clean_up() {
        let dir = scratch("temp");
        let dest = dir.join("x.jpg");
        let (a, b) = (TempFile::beside(&dest, "part"), TempFile::beside(&dest, "part"));
        assert_ne!(a.path(), b.path());
        fs::write(a.path(), b"partial").unwrap();
        let leftover = a.path().to_owned();
        drop(a);
        assert!(!leftover.exists());
    }

    #[derive(Default, Serialize, serde::Deserialize, PartialEq, Debug)]
    struct Store {
        #[serde(default)]
        names: Vec<String>,
    }

    impl Versioned for Store {
        const VERSION: u32 = 2;
        /// v0 kept one `name`; v1 made it a list under `names`.
        fn upgrade(json: &mut serde_json::Value, from: u32) -> Result<()> {
            if from == 0
                && let Some(map) = json.as_object_mut()
                && let Some(name) = map.remove("name")
            {
                map.insert("names".into(), serde_json::Value::Array(vec![name]));
            }
            Ok(())
        }
    }

    #[test]
    fn old_files_are_upgraded_and_kept() {
        let dir = scratch("upgrade");
        let path = dir.join("store.json");
        fs::write(&path, br#"{"name":"one"}"#).unwrap();
        let loaded: Store = load_versioned(&path);
        assert_eq!(loaded.names, ["one"]);
        assert!(dir.join("store.json.v0.bak").exists());
        save_versioned(&path, &loaded).unwrap();
        let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["version"], 2);
        assert_eq!(load_versioned::<Store>(&path), loaded);
    }

    #[test]
    fn newer_files_are_kept_before_anything_overwrites_them() {
        let dir = scratch("newer");
        let path = dir.join("store.json");
        fs::write(&path, br#"{"version":9,"names":["a"],"shiny":true}"#).unwrap();
        let loaded: Store = load_versioned(&path);
        assert_eq!(loaded.names, ["a"]);
        assert!(dir.join("store.json.v9.bak").exists());
    }

    #[test]
    fn corrupt_json_is_set_aside_not_lost() {
        let dir = scratch("corrupt");
        let path = dir.join("galleries.json");
        fs::write(&path, b"{ not json").unwrap();
        let loaded: Store = load_versioned(&path);
        assert!(loaded.names.is_empty());
        assert!(!path.exists());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
    }
}
