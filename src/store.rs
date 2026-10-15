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

/// Reads a JSON store. A missing file is the default. One that won't parse
/// is set aside as `<name>.corrupt-<time>` (and said so) rather than
/// replaced by an empty store on the next save.
pub fn load_json<T: DeserializeOwned + Default>(path: &Path) -> T {
    let Ok(text) = fs::read_to_string(path) else { return T::default() };
    match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            let aside = path.with_extension(format!("corrupt-{}", chrono::Utc::now().timestamp()));
            eprintln!("muralist: {} is unreadable ({e}); kept it as {}", path.display(), aside.display());
            let _ = fs::rename(path, &aside);
            T::default()
        }
    }
}

/// Saves a JSON store atomically.
pub fn save_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    write_atomic(path, &serde_json::to_vec(value)?)
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

    #[test]
    fn corrupt_json_is_set_aside_not_lost() {
        let dir = scratch("corrupt");
        let path = dir.join("galleries.json");
        fs::write(&path, b"{ not json").unwrap();
        let loaded: std::collections::BTreeMap<String, u32> = load_json(&path);
        assert!(loaded.is_empty());
        assert!(!path.exists());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
    }
}
