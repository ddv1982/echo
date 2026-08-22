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
}
