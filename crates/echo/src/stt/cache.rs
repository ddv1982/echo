use std::env;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ModelCache {
    dir: PathBuf,
}

impl ModelCache {
    #[must_use]
    pub fn from_env() -> Self {
        let dir = env::var_os("ECHO_MODEL_DIR")
            .map(PathBuf::from)
            .or_else(|| {
                env::var_os("XDG_CACHE_HOME").map(|cache| PathBuf::from(cache).join("echo"))
            })
            .or_else(|| {
                env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache").join("echo"))
            })
            .unwrap_or_else(|| PathBuf::from("/tmp/echo-models"));
        Self { dir }
    }

    #[must_use]
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    #[must_use]
    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    #[must_use]
    pub fn has(&self, name: &str) -> bool {
        self.path(name).is_file()
    }
}

pub fn on_path(name: &str) -> bool {
    env::var_os("PATH")
        .map(|paths| env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
        .unwrap_or(false)
}
