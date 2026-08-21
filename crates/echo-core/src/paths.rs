use std::env;
use std::path::PathBuf;

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
