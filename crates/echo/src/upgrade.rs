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
    DeferRestart,
    Restart,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupCleanup {
    Defer,
    TerminateStaleGui,
}

/// Startup takeover must not interrupt work owned by an existing GUI process.
#[must_use]
pub fn startup_cleanup_decision(recording_active: bool) -> StartupCleanup {
    if recording_active {
        StartupCleanup::Defer
    } else {
        StartupCleanup::TerminateStaleGui
    }
}

/// What a second launch should do. Same identity: the binary on disk is the
/// one running, so focus the window. Changed or missing identity: a package
/// upgrade replaced the binary, so the running process should hand over when
/// no recording is active. The caller guards loops by only exiting when the
/// fresh spawn succeeds.
#[must_use]
pub fn second_launch_decision(
    recorded: FileIdentity,
    current: Option<FileIdentity>,
    recording_active: bool,
) -> SecondLaunch {
    match current {
        Some(current) if current == recorded => SecondLaunch::Focus,
        _ if recording_active => SecondLaunch::DeferRestart,
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
    /// `/proc/<pid>/stat` field 22, stable for one process lifetime and changed
    /// when a numeric PID is reused.
    pub start_time_ticks: u64,
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
/// echo-desktop, or explicit work (`rec`, `transcribe`, or `--hud-demo`).
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
    let Some(argv0) = candidate.cmdline.first() else {
        return ProcessDisposition::Keep;
    };
    if argv0.is_empty() {
        return ProcessDisposition::Keep;
    }
    let args = candidate.cmdline.get(1..).unwrap_or(&[]);
    if args
        .iter()
        .any(|arg| arg == "rec" || arg == "transcribe" || arg == "--hud-demo")
    {
        return ProcessDisposition::Keep;
    }
    ProcessDisposition::Terminate
}

/// Old Echo GUI processes running under this uid, per the classifier.
#[must_use]
pub fn old_echo_processes() -> Vec<ProcessInfo> {
    use std::os::unix::fs::MetadataExt;
    let self_pid = std::process::id();
    let Ok(self_uid) = std::fs::metadata("/proc/self").map(|meta| meta.uid()) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return found;
    };
    for entry in entries.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let Some(info) = read_process_info(pid) else {
            continue;
        };
        if classify_process(&info, self_pid, self_uid) == ProcessDisposition::Terminate {
            found.push(info);
        }
    }
    found
}

fn read_process_info(pid: u32) -> Option<ProcessInfo> {
    use std::os::unix::fs::MetadataExt;

    let dir = std::path::PathBuf::from("/proc").join(pid.to_string());
    let exe = std::fs::read_link(dir.join("exe")).ok()?;
    let uid = std::fs::metadata(&dir).ok()?.uid();
    let stat = std::fs::read_to_string(dir.join("stat")).ok()?;
    let (state, start_time_ticks) = parse_process_stat(&stat)?;
    if state == 'Z' {
        return None;
    }
    // An unreadable, empty, or partially non-UTF-8 command line is not
    // evidence that the process is an idle GUI, so fail closed.
    let raw_cmdline = std::fs::read(dir.join("cmdline")).ok()?;
    let cmdline = parse_process_cmdline(&raw_cmdline)?;
    Some(ProcessInfo {
        pid,
        uid,
        start_time_ticks,
        exe: exe.to_string_lossy().into_owned(),
        cmdline,
    })
}

fn parse_process_cmdline(raw: &[u8]) -> Option<Vec<String>> {
    let mut tokens = raw.split(|byte| *byte == 0).collect::<Vec<_>>();
    // `/proc/<pid>/cmdline` normally ends in NUL. Remove only trailing
    // terminators so an explicitly empty argv[0] remains visible and causes
    // the classifier to fail closed.
    while matches!(tokens.last(), Some(token) if token.is_empty()) {
        tokens.pop();
    }
    if tokens.is_empty() || tokens.iter().all(|token| token.is_empty()) {
        return None;
    }
    tokens
        .into_iter()
        .map(|token| String::from_utf8(token.to_vec()))
        .collect::<Result<Vec<_>, _>>()
        .ok()
}

fn parse_process_stat(raw: &str) -> Option<(char, u64)> {
    let close = raw.rfind(") ")?;
    let mut fields = raw.get(close + 2..)?.split_whitespace();
    let state_text = fields.next()?;
    let mut state_chars = state_text.chars();
    let state = state_chars.next()?;
    if state_chars.next().is_some() {
        return None;
    }
    Some((state, fields.nth(18)?.parse().ok()?))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcessSignal {
    Term,
    Kill,
}

fn observe_signal_target(numeric_pid: u32) -> Option<(ProcessInfo, rustix::fd::OwnedFd)> {
    let pid = rustix::process::Pid::from_raw(numeric_pid.try_into().ok()?)?;
    // Open the handle first. Even if the numeric PID is reused after the /proc
    // re-read, the eventual signal remains pinned to this process lifetime.
    let target = rustix::process::pidfd_open(pid, rustix::process::PidfdFlags::empty()).ok()?;
    let process = read_process_info(numeric_pid)?;
    Some((process, target))
}

fn signal_process(target: rustix::fd::OwnedFd, signal: ProcessSignal) {
    let signal = match signal {
        ProcessSignal::Term => rustix::process::Signal::TERM,
        ProcessSignal::Kill => rustix::process::Signal::KILL,
    };
    let _ = rustix::process::pidfd_send_signal(target, signal);
}

fn same_signal_candidate(
    expected: &ProcessInfo,
    observed: &ProcessInfo,
    self_pid: u32,
    self_uid: u32,
) -> bool {
    expected == observed
        && classify_process(observed, self_pid, self_uid) == ProcessDisposition::Terminate
}

fn terminate_processes_with<T>(
    candidates: Vec<ProcessInfo>,
    self_pid: u32,
    self_uid: u32,
    mut observe: impl FnMut(u32) -> Option<(ProcessInfo, T)>,
    mut recording_busy: impl FnMut() -> bool,
    mut send_signal: impl FnMut(T, ProcessSignal),
    mut sleep: impl FnMut(std::time::Duration),
) {
    'candidates: for candidate in candidates {
        // This observation is intentionally adjacent to SIGTERM. A stale,
        // unreadable, reused, or newly-work-bearing PID is never signaled.
        let Some((current, target)) = observe(candidate.pid) else {
            continue;
        };
        if !same_signal_candidate(&candidate, &current, self_pid, self_uid) {
            continue;
        }
        if recording_busy() {
            break;
        }
        eprintln!(
            "echo-desktop: terminating old process {} ({})",
            candidate.pid, candidate.exe
        );
        send_signal(target, ProcessSignal::Term);

        for _ in 0..10 {
            sleep(std::time::Duration::from_millis(100));
            if recording_busy() {
                break 'candidates;
            }
            let Some((current, _target)) = observe(candidate.pid) else {
                continue 'candidates;
            };
            if !same_signal_candidate(&candidate, &current, self_pid, self_uid) {
                continue 'candidates;
            }
        }

        // Re-read both safety inputs at the executable SIGKILL boundary, even
        // though they were also checked during every grace-period poll.
        let Some((current, target)) = observe(candidate.pid) else {
            continue;
        };
        if !same_signal_candidate(&candidate, &current, self_pid, self_uid) {
            continue;
        }
        if recording_busy() {
            break;
        }
        send_signal(target, ProcessSignal::Kill);
    }
}

/// Terminate old Echo GUI processes: SIGTERM, a brief grace, then SIGKILL.
/// Runs once at desktop startup, before the tray is built.
pub fn terminate_old_echo_processes() {
    use std::os::unix::fs::MetadataExt;

    let Ok(self_uid) = std::fs::metadata("/proc/self").map(|meta| meta.uid()) else {
        return;
    };
    terminate_processes_with(
        old_echo_processes(),
        std::process::id(),
        self_uid,
        observe_signal_target,
        crate::rec::session_active,
        signal_process,
        std::thread::sleep,
    );
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RemovalReport {
    pub removed: Vec<std::path::PathBuf>,
    pub remaining: Vec<(std::path::PathBuf, String)>,
}

/// Delete the copies the scan classifies as stale right now, and only those.
/// When a stale binary was actually removed, also remove the known user-local
/// leftovers of a source install: the desktop entry and the hicolor icons.
/// Nothing here touches the running binary or needs privileges.
#[must_use]
pub fn remove_stale_installs(current: &Path, path_var: &str, home: &Path) -> RemovalReport {
    let mut report = RemovalReport::default();
    let Ok(current_id) = file_identity(current) else {
        return report;
    };
    let installs = path_installs(path_var);
    for path in stale_installs(&installs, current_id) {
        remove_one(&path, &mut report);
    }
    if report.removed.is_empty() {
        return report;
    }
    for leftover in user_local_leftovers(home) {
        if leftover.exists() {
            remove_one(&leftover, &mut report);
        }
    }
    report
}

fn remove_one(path: &Path, report: &mut RemovalReport) {
    match std::fs::remove_file(path) {
        Ok(()) => report.removed.push(path.to_path_buf()),
        Err(err) => report.remaining.push((path.to_path_buf(), err.to_string())),
    }
}

fn user_local_leftovers(home: &Path) -> Vec<std::path::PathBuf> {
    let share = home.join(".local").join("share");
    let mut paths = vec![
        share.join("applications/Echo.desktop"),
        share.join("icons/hicolor/scalable/apps/echo-desktop.svg"),
        share.join("icons/hicolor/symbolic/apps/echo-desktop-symbolic.svg"),
    ];
    for size in [32, 128, 256, 512] {
        paths.push(share.join(format!("icons/hicolor/{size}x{size}/apps/echo-desktop.png")));
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    struct PidfdTestChild {
        root: std::path::PathBuf,
        child: std::process::Child,
        reaped: bool,
    }

    #[cfg(target_os = "linux")]
    impl PidfdTestChild {
        fn spawn() -> Result<Self, String> {
            use std::sync::atomic::{AtomicU64, Ordering};

            static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

            let source = [
                std::path::Path::new("/usr/bin/sleep"),
                std::path::Path::new("/bin/sleep"),
            ]
            .into_iter()
            .find(|path| path.is_file())
            .ok_or_else(|| {
                "pidfd integration test requires /usr/bin/sleep or /bin/sleep".to_string()
            })?;

            let mut root = None;
            for _ in 0..100 {
                let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
                let candidate = std::env::temp_dir()
                    .join(format!("echo-pidfd-test-{}-{id}", std::process::id()));
                match std::fs::create_dir(&candidate) {
                    Ok(()) => {
                        root = Some(candidate);
                        break;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => {
                        return Err(format!(
                            "cannot create pidfd integration test directory {}: {error}",
                            candidate.display()
                        ));
                    }
                }
            }
            let root = root.ok_or_else(|| {
                "cannot create a unique pidfd integration test directory after 100 attempts"
                    .to_string()
            })?;

            let executable = root.join("echo-desktop");
            if let Err(error) = std::fs::copy(source, &executable) {
                let _ = std::fs::remove_dir_all(&root);
                return Err(format!(
                    "cannot copy {} to {} for pidfd integration test: {error}",
                    source.display(),
                    executable.display()
                ));
            }
            let child = match std::process::Command::new(&executable).arg("60").spawn() {
                Ok(child) => child,
                Err(error) => {
                    let _ = std::fs::remove_dir_all(&root);
                    return Err(format!(
                        "cannot execute controlled {} child for pidfd integration test: {error}",
                        executable.display()
                    ));
                }
            };
            Ok(Self {
                root,
                child,
                reaped: false,
            })
        }

        fn wait_for_exit(
            &mut self,
            timeout: std::time::Duration,
        ) -> Result<std::process::ExitStatus, String> {
            let deadline = std::time::Instant::now() + timeout;
            loop {
                match self.child.try_wait() {
                    Ok(Some(status)) => {
                        self.reaped = true;
                        return Ok(status);
                    }
                    Ok(None) if std::time::Instant::now() < deadline => {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                    }
                    Ok(None) => {
                        return Err(format!(
                            "controlled child {} did not exit within {timeout:?} after pidfd SIGTERM",
                            self.child.id()
                        ));
                    }
                    Err(error) => {
                        return Err(format!(
                            "cannot wait for controlled child {} after pidfd SIGTERM: {error}",
                            self.child.id()
                        ));
                    }
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    impl Drop for PidfdTestChild {
        fn drop(&mut self) {
            if !self.reaped {
                match self.child.try_wait() {
                    Ok(Some(_)) => self.reaped = true,
                    Ok(None) | Err(_) => {
                        let _ = self.child.kill();
                        let _ = self.child.wait();
                        self.reaped = true;
                    }
                }
            }
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn id(dev: u64, ino: u64) -> FileIdentity {
        FileIdentity { dev, ino }
    }

    #[test]
    fn same_identity_focuses() {
        assert_eq!(
            second_launch_decision(id(1, 2), Some(id(1, 2)), false),
            SecondLaunch::Focus
        );
        assert_eq!(
            second_launch_decision(id(1, 2), Some(id(1, 2)), true),
            SecondLaunch::Focus,
            "recording state does not change a same-binary focus"
        );
    }

    #[test]
    fn changed_inode_or_device_restarts() {
        assert_eq!(
            second_launch_decision(id(1, 2), Some(id(1, 3)), false),
            SecondLaunch::Restart
        );
        assert_eq!(
            second_launch_decision(id(1, 2), Some(id(2, 2)), false),
            SecondLaunch::Restart
        );
    }

    #[test]
    fn missing_file_restarts_so_the_spawn_guard_decides() {
        assert_eq!(
            second_launch_decision(id(1, 2), None, false),
            SecondLaunch::Restart
        );
    }

    #[test]
    fn startup_cleanup_preserves_active_gui_work_and_terminates_idle_stale_gui() {
        assert_eq!(startup_cleanup_decision(true), StartupCleanup::Defer);
        assert_eq!(
            startup_cleanup_decision(false),
            StartupCleanup::TerminateStaleGui
        );
    }

    #[test]
    fn changed_binary_restart_is_deferred_only_while_recording() {
        assert_eq!(
            second_launch_decision(id(1, 2), Some(id(1, 3)), true),
            SecondLaunch::DeferRestart
        );
        assert_eq!(
            second_launch_decision(id(1, 2), Some(id(1, 3)), false),
            SecondLaunch::Restart
        );
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
        let canonical_early = early.canonicalize().unwrap();
        assert_eq!(installs.len(), 2, "only echo-desktop files, once each");
        assert!(
            installs[0].0.starts_with(&canonical_early),
            "PATH order preserved"
        );

        let current = file_identity(&late.join("echo-desktop")).unwrap();
        let stale = stale_installs(&installs, current);
        assert_eq!(stale.len(), 1);
        assert!(stale[0].starts_with(&canonical_early));
        assert!(stale_installs(&installs[1..], current).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    fn process(pid: u32, uid: u32, exe: &str, cmdline: &[&str]) -> ProcessInfo {
        ProcessInfo {
            pid,
            uid,
            start_time_ticks: 1234,
            exe: exe.to_string(),
            cmdline: cmdline.iter().map(|arg| arg.to_string()).collect(),
        }
    }

    #[test]
    fn process_stat_parser_handles_parentheses_and_spaces_in_comm() {
        let mut trailing = vec!["0"; 18];
        trailing.push("998877");
        let raw = format!("22 (old echo (desktop)) S {}", trailing.join(" "));
        assert_eq!(parse_process_stat(&raw), Some(('S', 998877)));
    }

    #[test]
    fn empty_cmdline_fails_closed_before_process_signaling() {
        assert_eq!(parse_process_cmdline(b""), None);
        assert_eq!(parse_process_cmdline(b"\0\0"), None);

        let empty = process(100, 1000, "/usr/bin/echo-desktop", &[]);
        assert_eq!(
            classify_process(&empty, 200, 1000),
            ProcessDisposition::Keep
        );

        let empty_argv0 = process(100, 1000, "/usr/bin/echo-desktop", &["", "60"]);
        assert_eq!(
            classify_process(&empty_argv0, 200, 1000),
            ProcessDisposition::Keep
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn real_pidfd_signal_reaches_the_pinned_controlled_child() -> Result<(), String> {
        use std::os::unix::fs::MetadataExt;
        use std::os::unix::process::ExitStatusExt;

        let mut controlled = PidfdTestChild::spawn()?;
        let pid = controlled.child.id();
        let self_uid = std::fs::metadata("/proc/self")
            .map_err(|error| format!("cannot read /proc/self for pidfd integration test: {error}"))?
            .uid();

        let initial = read_process_info(pid).ok_or_else(|| {
            format!("cannot read controlled child {pid} from /proc after successful spawn")
        })?;
        assert_eq!(initial.pid, pid);
        assert_eq!(
            std::path::Path::new(&initial.exe).file_name(),
            Some(std::ffi::OsStr::new("echo-desktop"))
        );
        assert!(!initial.cmdline.is_empty());
        assert_eq!(
            classify_process(&initial, std::process::id(), self_uid),
            ProcessDisposition::Terminate,
            "the controlled child must have an idle, non-protected command line"
        );

        let (observed, target) = observe_signal_target(pid).ok_or_else(|| {
            format!(
                "cannot open and observe a real pidfd for controlled child {pid}; Linux pidfd support is required"
            )
        })?;
        assert_eq!(
            observed, initial,
            "the pidfd must be paired with the same observed process lifetime"
        );

        signal_process(target, ProcessSignal::Term);
        let status = controlled.wait_for_exit(std::time::Duration::from_secs(5))?;
        assert_eq!(
            status.signal(),
            Some(15),
            "the Child handle for pid {pid} must observe the TERM sent through its pinned pidfd"
        );
        Ok(())
    }

    #[test]
    fn lock_activation_during_term_grace_suppresses_kill() {
        use std::cell::Cell;

        let candidate = process(100, 1000, "/usr/bin/echo-desktop", &["echo-desktop"]);
        let busy = Cell::new(false);
        let mut signals = Vec::new();
        terminate_processes_with(
            vec![candidate.clone()],
            200,
            1000,
            |_| Some((candidate.clone(), 100)),
            || busy.get(),
            |pid, signal| signals.push((pid, signal)),
            |_| busy.set(true),
        );
        assert_eq!(signals, vec![(100, ProcessSignal::Term)]);
    }

    #[test]
    fn reused_pid_at_kill_boundary_is_not_signaled_again() {
        let candidate = process(100, 1000, "/usr/bin/echo-desktop", &["echo-desktop"]);
        let mut replacement = candidate.clone();
        replacement.start_time_ticks += 1;
        let mut observation_count = 0;
        let mut signals = Vec::new();
        terminate_processes_with(
            vec![candidate.clone()],
            200,
            1000,
            |_| {
                observation_count += 1;
                if observation_count <= 11 {
                    Some((candidate.clone(), 100))
                } else {
                    Some((replacement.clone(), 100))
                }
            },
            || false,
            |pid, signal| signals.push((pid, signal)),
            |_| {},
        );
        assert_eq!(observation_count, 12, "identity was read again before KILL");
        assert_eq!(signals, vec![(100, ProcessSignal::Term)]);
    }

    #[test]
    fn stable_idle_candidate_reaches_term_and_kill_boundaries() {
        let candidate = process(100, 1000, "/usr/bin/echo-desktop", &["echo-desktop"]);
        let mut signals = Vec::new();
        terminate_processes_with(
            vec![candidate.clone()],
            200,
            1000,
            |_| Some((candidate.clone(), 100)),
            || false,
            |pid, signal| signals.push((pid, signal)),
            |_| {},
        );
        assert_eq!(
            signals,
            vec![(100, ProcessSignal::Term), (100, ProcessSignal::Kill)]
        );
    }

    #[test]
    fn cmdline_change_or_unreadable_candidate_suppresses_term() {
        let candidate = process(100, 1000, "/usr/bin/echo-desktop", &["echo-desktop"]);
        let mut changed = candidate.clone();
        changed.cmdline.push("rec".to_string());
        let mut signals = Vec::new();
        terminate_processes_with(
            vec![candidate.clone()],
            200,
            1000,
            |_| Some((changed.clone(), 100)),
            || false,
            |pid, signal| signals.push((pid, signal)),
            |_| {},
        );
        terminate_processes_with(
            vec![candidate],
            200,
            1000,
            |_| None,
            || false,
            |pid, signal| signals.push((pid, signal)),
            |_| {},
        );
        assert!(signals.is_empty());
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
        let stranger = process(
            100,
            1000,
            "/usr/bin/echo-desktop-not",
            &["echo-desktop-not"],
        );
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
    fn classifier_keeps_recorders_transcriptions_and_demos() {
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
        let transcription = process(
            100,
            1000,
            "/usr/bin/echo-desktop",
            &["echo-desktop", "transcribe", "recording.wav"],
        );
        assert_eq!(
            classify_process(&transcription, 200, 1000),
            ProcessDisposition::Keep,
            "an explicit transcription must not be killed"
        );
        let demo = process(
            100,
            1000,
            "/tmp/echo-desktop",
            &["echo-desktop", "--hud-demo"],
        );
        assert_eq!(classify_process(&demo, 200, 1000), ProcessDisposition::Keep);
    }

    #[test]
    fn removal_deletes_stale_copies_and_user_local_leftovers() {
        let root = std::env::temp_dir().join(format!("echo-remove-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let bin_dir = root.join("bin");
        let home = root.join("home");
        std::fs::create_dir_all(&bin_dir).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        let current = root.join("current");
        std::fs::write(&current, b"new").unwrap();
        let stale = bin_dir.join("echo-desktop");
        std::fs::write(&stale, b"old").unwrap();
        let entry = home.join(".local/share/applications/Echo.desktop");
        std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
        std::fs::write(&entry, b"[Desktop Entry]").unwrap();
        let icon = home.join(".local/share/icons/hicolor/scalable/apps/echo-desktop.svg");
        std::fs::create_dir_all(icon.parent().unwrap()).unwrap();
        std::fs::write(&icon, b"<svg/>").unwrap();

        let report = remove_stale_installs(&current, &bin_dir.to_string_lossy(), &home);
        assert!(report.remaining.is_empty());
        assert!(!stale.exists(), "stale binary removed");
        assert!(!entry.exists(), "user-local desktop entry removed");
        assert!(!icon.exists(), "user-local icon removed");
        assert!(current.exists(), "the running binary is never touched");
        assert_eq!(report.removed.len(), 3);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn removal_without_a_stale_binary_leaves_leftovers_alone() {
        let root = std::env::temp_dir().join(format!("echo-remove-none-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let home = root.join("home");
        let entry = home.join(".local/share/applications/Echo.desktop");
        std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
        std::fs::write(&entry, b"[Desktop Entry]").unwrap();
        let current = root.join("current");
        std::fs::write(&current, b"new").unwrap();

        let report = remove_stale_installs(&current, "", &home);
        assert!(report.removed.is_empty());
        assert!(
            entry.exists(),
            "leftovers stay when no stale binary was removed"
        );
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
