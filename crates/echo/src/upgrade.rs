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

/// What we know about one process from /proc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: u32,
    pub uid: u32,
    /// The `/proc/<pid>/exe` link text. After an upgrade replaced the binary
    /// this reads with a ` (deleted)` suffix; the file name still matches.
    pub exe: String,
    pub cmdline: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessDisposition {
    Keep,
    Terminate,
}

/// Old Echo processes predate the single-instance gate, so a new launch would
/// otherwise coexist with them: two trays, and the upgrade looking like it
/// never happened. Classify a candidate for takeover at startup. Never touch
/// ourselves, other users' processes, anything whose executable is not named
/// echo-desktop, or a recorder mid-dictation (`rec` subcommand, `--hud-demo`).
#[must_use]
pub fn classify_process(
    candidate: &ProcessInfo,
    self_pid: u32,
    self_uid: u32,
) -> ProcessDisposition {
    if candidate.pid == self_pid || candidate.uid != self_uid {
        return ProcessDisposition::Keep;
    }
    let exe_text = candidate
        .exe
        .strip_suffix(" (deleted)")
        .unwrap_or(&candidate.exe);
    let is_echo = std::path::Path::new(exe_text)
        .file_name()
        .map(|name| name == "echo-desktop")
        .unwrap_or(false);
    if !is_echo {
        return ProcessDisposition::Keep;
    }
    let args = candidate.cmdline.get(1..).unwrap_or(&[]);
    if args.iter().any(|arg| arg == "rec" || arg == "--hud-demo") {
        return ProcessDisposition::Keep;
    }
    ProcessDisposition::Terminate
}

/// Old Echo GUI processes running under this uid, per the classifier.
#[must_use]
pub fn old_echo_processes() -> Vec<ProcessInfo> {
    use std::os::unix::fs::MetadataExt;
    let self_pid = std::process::id();
    let self_uid = std::fs::metadata("/proc/self")
        .map(|meta| meta.uid())
        .unwrap_or(0);
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return found;
    };
    for entry in entries.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let dir = entry.path();
        let Ok(exe) = std::fs::read_link(dir.join("exe")) else {
            continue;
        };
        let Ok(uid) = std::fs::metadata(&dir).map(|meta| meta.uid()) else {
            continue;
        };
        let cmdline = std::fs::read(dir.join("cmdline"))
            .map(|raw| {
                raw.split(|byte| *byte == 0)
                    .filter_map(|token| String::from_utf8(token.to_vec()).ok())
                    .filter(|token| !token.is_empty())
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default();
        let info = ProcessInfo {
            pid,
            uid,
            exe: exe.to_string_lossy().into_owned(),
            cmdline,
        };
        if classify_process(&info, self_pid, self_uid) == ProcessDisposition::Terminate {
            found.push(info);
        }
    }
    found
}

fn process_alive(pid: u32) -> bool {
    // A zombie still has a /proc entry; its state letter is Z.
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(stat) => !stat.contains(") Z"),
        Err(_) => false,
    }
}

fn signal(pid: u32, signal: &str) {
    // Spawning kill keeps unsafe out of the workspace; the nix crate is the
    // rejected alternative for two signals at startup.
    let _ = std::process::Command::new("kill")
        .arg(signal)
        .arg(pid.to_string())
        .status();
}

/// Terminate old Echo GUI processes: SIGTERM, a brief grace, then SIGKILL.
/// Runs once at desktop startup, before the tray is built.
pub fn terminate_old_echo_processes() {
    for process in old_echo_processes() {
        eprintln!(
            "echo-desktop: terminating old process {} ({})",
            process.pid, process.exe
        );
        signal(process.pid, "-TERM");
        for _ in 0..10 {
            if !process_alive(process.pid) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        if process_alive(process.pid) {
            signal(process.pid, "-KILL");
        }
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

    fn process(pid: u32, uid: u32, exe: &str, cmdline: &[&str]) -> ProcessInfo {
        ProcessInfo {
            pid,
            uid,
            exe: exe.to_string(),
            cmdline: cmdline.iter().map(|arg| arg.to_string()).collect(),
        }
    }

    #[test]
    fn classifier_keeps_self_other_users_and_strangers() {
        let gui = process(100, 1000, "/usr/bin/echo-desktop", &["echo-desktop"]);
        assert_eq!(
            classify_process(&gui, 100, 1000),
            ProcessDisposition::Keep,
            "never kill ourselves"
        );
        assert_eq!(
            classify_process(&gui, 200, 999),
            ProcessDisposition::Keep,
            "never touch another user's processes"
        );
        let stranger = process(100, 1000, "/usr/bin/echo-desktop-not", &["echo-desktop-not"]);
        assert_eq!(
            classify_process(&stranger, 200, 1000),
            ProcessDisposition::Keep
        );
    }

    #[test]
    fn classifier_terminates_old_gui_processes() {
        let old = process(100, 1000, "/usr/bin/echo-desktop", &["echo-desktop"]);
        assert_eq!(
            classify_process(&old, 200, 1000),
            ProcessDisposition::Terminate
        );
        let deleted = process(
            100,
            1000,
            "/usr/bin/echo-desktop (deleted)",
            &["/usr/bin/echo-desktop"],
        );
        assert_eq!(
            classify_process(&deleted, 200, 1000),
            ProcessDisposition::Terminate,
            "an upgraded-away binary still matches by file name"
        );
    }

    #[test]
    fn classifier_keeps_recorders_and_demos() {
        let recorder = process(
            100,
            1000,
            "/usr/bin/echo-desktop",
            &["echo-desktop", "rec", "--toggle"],
        );
        assert_eq!(
            classify_process(&recorder, 200, 1000),
            ProcessDisposition::Keep,
            "an active dictation must not be killed"
        );
        let demo = process(100, 1000, "/tmp/echo-desktop", &["echo-desktop", "--hud-demo"]);
        assert_eq!(
            classify_process(&demo, 200, 1000),
            ProcessDisposition::Keep
        );
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
