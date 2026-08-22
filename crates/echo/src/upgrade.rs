use std::io;
use std::path::Path;

/// Device plus inode: the identity of the file a running process was loaded
/// from. A package upgrade replaces the file, which changes the identity even
/// though the path stays the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileIdentity {
    pub dev: u64,
    pub ino: u64,
}

pub fn file_identity(path: &Path) -> io::Result<FileIdentity> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(path)?;
    Ok(FileIdentity {
        dev: meta.dev(),
        ino: meta.ino(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecondLaunch {
    Focus,
    Restart,
}

/// What a second launch should do. Same identity: the binary on disk is the
/// one running, so focus the window. Changed or missing identity: a package
/// upgrade replaced the binary, so the running process should hand over to
/// the on-disk build and exit. The caller guards loops by only exiting when
/// the fresh spawn succeeds.
#[must_use]
pub fn second_launch_decision(
    recorded: FileIdentity,
    current: Option<FileIdentity>,
) -> SecondLaunch {
    match current {
        Some(current) if current == recorded => SecondLaunch::Focus,
        _ => SecondLaunch::Restart,
    }
}

/// Every `echo-desktop` reachable through `path_var` (a PATH-style list),
/// canonicalized and deduplicated by file identity, in PATH order.
#[must_use]
pub fn path_installs(path_var: &str) -> Vec<(std::path::PathBuf, FileIdentity)> {
    let mut installs: Vec<(std::path::PathBuf, FileIdentity)> = Vec::new();
    for dir in std::env::split_paths(path_var) {
        let candidate = dir.join("echo-desktop");
        let Ok(canonical) = candidate.canonicalize() else {
            continue;
        };
        let Ok(identity) = file_identity(&canonical) else {
            continue;
        };
        if !installs.iter().any(|(_, id)| *id == identity) {
            installs.push((canonical, identity));
        }
    }
    installs
}

/// Copies on PATH whose identity differs from the running binary: stale
/// installs that shadow or confuse. The first PATH hit decides what a
/// bare `echo-desktop` launch runs.
#[must_use]
pub fn stale_installs(
    installs: &[(std::path::PathBuf, FileIdentity)],
    current: FileIdentity,
) -> Vec<std::path::PathBuf> {
    installs
        .iter()
        .filter(|(_, identity)| *identity != current)
        .map(|(path, _)| path.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(dev: u64, ino: u64) -> FileIdentity {
        FileIdentity { dev, ino }
    }

    #[test]
    fn same_identity_focuses() {
        assert_eq!(
            second_launch_decision(id(1, 2), Some(id(1, 2))),
            SecondLaunch::Focus
        );
    }

    #[test]
    fn changed_inode_or_device_restarts() {
        assert_eq!(
            second_launch_decision(id(1, 2), Some(id(1, 3))),
            SecondLaunch::Restart
        );
        assert_eq!(
            second_launch_decision(id(1, 2), Some(id(2, 2))),
            SecondLaunch::Restart
        );
    }

    #[test]
    fn missing_file_restarts_so_the_spawn_guard_decides() {
        assert_eq!(second_launch_decision(id(1, 2), None), SecondLaunch::Restart);
    }

    #[test]
    fn rename_over_changes_the_identity() {
        // dpkg and rpm install by renaming a fresh file over the old path.
        let dir = std::env::temp_dir().join(format!("echo-upgrade-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("echo-desktop");
        std::fs::write(&path, b"old").unwrap();
        let before = file_identity(&path).unwrap();
        let staged = dir.join("echo-desktop.new");
        std::fs::write(&staged, b"new").unwrap();
        std::fs::rename(&staged, &path).unwrap();
        let after = file_identity(&path).unwrap();
        assert_ne!(before, after, "replace-on-upgrade must change identity");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn path_scan_finds_installs_in_path_order_and_stale_ones_differ() {
        let root = std::env::temp_dir().join(format!("echo-path-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let early = root.join("local-bin");
        let late = root.join("usr-bin");
        std::fs::create_dir_all(&early).unwrap();
        std::fs::create_dir_all(&late).unwrap();
        std::fs::write(early.join("echo-desktop"), b"stale").unwrap();
        std::fs::write(late.join("echo-desktop"), b"current").unwrap();
        std::fs::write(late.join("not-echo"), b"other").unwrap();
        let path_var = format!("{}:{}", early.display(), late.display());

        let installs = path_installs(&path_var);
        assert_eq!(installs.len(), 2, "only echo-desktop files, once each");
        assert!(installs[0].0.starts_with(&early), "PATH order preserved");

        let current = file_identity(&late.join("echo-desktop")).unwrap();
        let stale = stale_installs(&installs, current);
        assert_eq!(stale.len(), 1);
        assert!(stale[0].starts_with(&early));
        assert!(stale_installs(&installs[1..], current).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn path_scan_dedupes_symlinked_dirs() {
        let root = std::env::temp_dir().join(format!("echo-path-dedup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let real = root.join("bin");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("echo-desktop"), b"one").unwrap();
        let alias = root.join("alias");
        std::os::unix::fs::symlink(&real, &alias).unwrap();
        let path_var = format!("{}:{}", real.display(), alias.display());
        assert_eq!(path_installs(&path_var).len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }
}
