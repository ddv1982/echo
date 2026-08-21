use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[must_use]
pub fn data_dir() -> PathBuf {
    if let Some(dir) = env::var_os("ECHO_DATA_DIR") {
        return PathBuf::from(dir);
    }
    if let Some(xdg) = env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(xdg).join("echo");
    }
    if let Some(home) = env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("echo");
    }
    PathBuf::from("/tmp/echo-data")
}

#[must_use]
pub fn dictionary_path() -> PathBuf {
    data_dir().join("dictionary.json")
}

#[must_use]
pub fn history_path() -> PathBuf {
    data_dir().join("history.json")
}

#[must_use]
pub fn status_path() -> PathBuf {
    data_dir().join("status")
}

/// Move an unparseable store aside so the app can start fresh without
/// destroying the evidence.
pub(crate) fn set_aside_corrupt(path: &Path) {
    if let (Some(parent), Some(name)) = (path.parent(), path.file_name().and_then(|n| n.to_str()))
    {
        let _ = fs::rename(path, parent.join(format!("{name}.corrupt")));
    }
}

/// Write via a same-directory temp file plus rename, so a crash mid-write
/// never corrupts the previous contents and readers never see a partial file.
pub fn write_atomic(path: &Path, contents: &[u8]) -> Result<(), String> {
    // Pid alone is not unique enough: two threads of the desktop app can
    // write the same file concurrently, so each call gets its own counter.
    static WRITE_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = WRITE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let parent = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .ok_or_else(|| format!("{} has no parent directory", path.display()))?;
    fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("{} has no file name", path.display()))?;
    let tmp = parent.join(format!(".{name}.tmp-{}-{seq}", std::process::id()));
    fs::write(&tmp, contents).map_err(|err| err.to_string())?;
    fs::rename(&tmp, path).map_err(|err| err.to_string())
}
