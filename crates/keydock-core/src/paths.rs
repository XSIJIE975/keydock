use std::path::{Path, PathBuf};

pub const BUNDLE_IDENTIFIER: &str = "com.xsjie.keydock";
pub const CONFIG_FILE: &str = "config.json";
pub const VAULT_FILE: &str = "vault.enc";
pub const CHANGELOG_FILE: &str = "changelog.json";

#[derive(Debug, thiserror::Error)]
pub enum PathError {
    #[error("the operating system did not provide a per-user data directory")]
    NoDataDir,
    #[error("data directory is not writable: {0}")]
    NotWritable(PathBuf),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
}

/// `%APPDATA%\com.xsjie.keydock` on Windows and
/// `~/Library/Application Support/com.xsjie.keydock` on macOS.
pub fn data_dir() -> Result<PathBuf, PathError> {
    dirs::data_dir()
        .map(|dir| dir.join(BUNDLE_IDENTIFIER))
        .ok_or(PathError::NoDataDir)
}

pub fn config_path(dir: &Path) -> PathBuf {
    dir.join(CONFIG_FILE)
}

pub fn vault_path(dir: &Path) -> PathBuf {
    dir.join(VAULT_FILE)
}

/// Temporary browser profiles live below the data directory and are swept on
/// application start.
pub fn sessions_root(dir: &Path) -> PathBuf {
    dir.join(".tmp").join("sessions")
}

pub fn ensure_data_dir(dir: &Path) -> Result<(), PathError> {
    std::fs::create_dir_all(dir)?;
    let probe = dir.join(".write-probe");
    std::fs::write(&probe, b"keydock")?;
    std::fs::remove_file(&probe)?;
    Ok(())
}

/// Create a fresh, empty session profile directory.
pub fn create_session_dir(root: &Path, session_id: uuid::Uuid) -> Result<PathBuf, PathError> {
    let path = root.join(session_id.to_string());
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

/// Remove every leftover session directory. Called once at start-up while the
/// single-instance lock guarantees no live session exists.
pub fn sweep_sessions(root: &Path) -> Result<usize, PathError> {
    if !root.exists() {
        return Ok(0);
    }
    let mut removed = 0;
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        let is_portable_name = entry
            .file_name()
            .to_str()
            .map(|name| {
                name.chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
            })
            .unwrap_or(false);
        if !is_portable_name {
            continue;
        }
        if path.is_dir() {
            match std::fs::remove_dir_all(&path) {
                Ok(()) => removed += 1,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(PathError::Io(error)),
            }
        }
    }
    Ok(removed)
}

/// Write a file atomically: write `.tmp`, fsync, then rename over the target.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or_default()
    ));
    {
        use std::io::Write;
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    match std::fs::rename(&tmp, path) {
        Ok(()) => {}
        Err(first) => {
            // Antivirus/indexers occasionally hold the destination briefly.
            std::thread::sleep(std::time::Duration::from_millis(50));
            std::fs::rename(&tmp, path).map_err(|_| first)?;
        }
    }
    if let Ok(dir) = std::fs::File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("keydock-core-{name}-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn data_dir_ends_with_bundle_identifier() {
        let dir = data_dir().expect("data dir");
        assert!(dir.ends_with(BUNDLE_IDENTIFIER));
    }

    #[test]
    fn atomic_write_replaces_existing_and_leaves_no_tmp() {
        let root = temp_root("atomic");
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("config.json");
        atomic_write(&path, b"one").unwrap();
        atomic_write(&path, b"two").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"two");
        assert!(!root.join("config.json.tmp").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sweep_removes_only_session_directories() {
        let root = temp_root("sweep");
        std::fs::create_dir_all(root.join("a")).unwrap();
        std::fs::create_dir_all(root.join("..sneaky")).unwrap();
        std::fs::write(root.join("readme.txt"), b"keep").unwrap();
        let removed = sweep_sessions(&root).unwrap();
        assert_eq!(removed, 1);
        assert!(root.join("readme.txt").exists());
        assert!(root.join("..sneaky").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
