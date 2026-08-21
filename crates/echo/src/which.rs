use std::env;

/// True when `name` is a file on `PATH`.
#[must_use]
pub fn on_path(name: &str) -> bool {
    env::var_os("PATH")
        .map(|paths| env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
        .unwrap_or(false)
}
