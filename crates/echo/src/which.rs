use std::env;
use std::path::PathBuf;

/// True when `name` is a file on `PATH`.
#[must_use]
pub fn on_path(name: &str) -> bool {
    path_of(name).is_some()
}

/// The resolved path of `name` on `PATH`, for readouts that show what ran.
#[must_use]
pub fn path_of(name: &str) -> Option<PathBuf> {
    env::var_os("PATH").and_then(|paths| {
        env::split_paths(&paths)
            .map(|dir| dir.join(name))
            .find(|path| path.is_file())
    })
}
