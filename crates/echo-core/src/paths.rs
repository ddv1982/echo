use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[must_use]
pub fn data_dir() -> PathBuf {
    resolve_dir(
        env::var_os("ECHO_DATA_DIR").map(PathBuf::from),
        env::var_os("XDG_DATA_HOME").map(PathBuf::from),
        env::var_os("HOME").map(PathBuf::from),
        &[".local", "share", "echo"],
        "/tmp/echo-data",
    )
}

#[must_use]
pub fn config_dir() -> PathBuf {
    resolve_dir(
        env::var_os("ECHO_CONFIG_DIR").map(PathBuf::from),
        env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
        env::var_os("HOME").map(PathBuf::from),
        &[".config", "echo"],
        "/tmp/echo-config",
    )
}

fn resolve_dir(
    explicit: Option<PathBuf>,
    xdg: Option<PathBuf>,
    home: Option<PathBuf>,
    under_home: &[&str],
    fallback: &str,
) -> PathBuf {
    if let Some(dir) = explicit {
        return dir;
    }
    if let Some(xdg) = xdg {
        if xdg.is_absolute() {
            return xdg.join("echo");
        }
    }
    if let Some(home) = home {
        let mut dir = home;
        for part in under_home {
            dir.push(part);
        }
        return dir;
    }
    PathBuf::from(fallback)
}

#[must_use]
pub fn config_path() -> PathBuf {
    config_dir().join("config.json")
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
    if let (Some(parent), Some(name)) = (path.parent(), path.file_name().and_then(|n| n.to_str())) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_xdg_config_home_falls_back_to_home_config() {
        let got = resolve_dir(
            None,
            Some(PathBuf::from("")),
            Some(PathBuf::from("/home/tester")),
            &[".config", "echo"],
            "/tmp/echo-config",
        );
        assert_eq!(got, PathBuf::from("/home/tester/.config/echo"));
    }

    #[test]
    fn relative_xdg_config_home_falls_back_to_home_config() {
        let got = resolve_dir(
            None,
            Some(PathBuf::from("relative/xdg")),
            Some(PathBuf::from("/home/tester")),
            &[".config", "echo"],
            "/tmp/echo-config",
        );
        assert_eq!(got, PathBuf::from("/home/tester/.config/echo"));
    }

    #[test]
    fn empty_xdg_data_home_falls_back_to_home_local_share() {
        let got = resolve_dir(
            None,
            Some(PathBuf::from("")),
            Some(PathBuf::from("/home/tester")),
            &[".local", "share", "echo"],
            "/tmp/echo-data",
        );
        assert_eq!(got, PathBuf::from("/home/tester/.local/share/echo"));
    }

    #[test]
    fn relative_xdg_data_home_falls_back_to_home_local_share() {
        let got = resolve_dir(
            None,
            Some(PathBuf::from("relative/xdg")),
            Some(PathBuf::from("/home/tester")),
            &[".local", "share", "echo"],
            "/tmp/echo-data",
        );
        assert_eq!(got, PathBuf::from("/home/tester/.local/share/echo"));
    }
}
