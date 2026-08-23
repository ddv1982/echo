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
            .find(|path| executable_file(path))
    })
}

fn executable_file(path: &std::path::Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::executable_file;

    #[cfg(unix)]
    #[test]
    fn non_executable_path_entries_are_not_runtimes() {
        use std::os::unix::fs::PermissionsExt;

        let path = std::env::temp_dir().join(format!("echo-runtime-mode-{}", std::process::id()));
        std::fs::write(&path, b"#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(!executable_file(&path));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(executable_file(&path));
        std::fs::remove_file(path).unwrap();
    }
}
