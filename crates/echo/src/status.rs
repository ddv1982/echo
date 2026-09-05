use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use echo_core::{status_path, write_atomic_private, PrivateDir, RecordingLimit, SessionState};

use crate::process_identity::{self, ProcessIdentity};

/// Status file contents after staleness handling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub state: String,
    pub last: Option<String>,
    pub last_history_id: Option<String>,
    /// Actionable failure detail from the session, including engine and local
    /// persistence errors, so the desktop app can expose the actual problem.
    pub error: Option<String>,
    pub recording_limit: Option<RecordingLimit>,
    pub session_id: Option<String>,
    pub revision: u64,
}

impl Status {
    #[must_use]
    pub fn idle() -> Self {
        Self {
            state: "Idle".to_string(),
            last: None,
            last_history_id: None,
            error: None,
            recording_limit: None,
            session_id: None,
            revision: 0,
        }
    }
}

#[must_use]
pub fn state_name(state: SessionState) -> String {
    match state {
        SessionState::Idle => "Idle".to_string(),
        SessionState::Recording { .. } => "Recording".to_string(),
        SessionState::Transcribing => "Transcribing".to_string(),
        SessionState::Injecting => "Injecting".to_string(),
        SessionState::Failed { reason } => format!("Failed {}", reason.as_str()),
    }
}

/// Persist the session state for other processes. The file carries the
/// writer's pid so readers can spot a session whose process died.
pub fn write_status(
    state: SessionState,
    last: Option<&str>,
    error: Option<&str>,
    last_history_id: Option<&str>,
) -> Result<(), String> {
    write_atomic_private(
        &status_path(),
        render(state, last, error, last_history_id).as_bytes(),
    )
}

/// Only the recording owner calls this while it holds the lease. Keeping the
/// identity in the same atomic status file prevents a reader from combining
/// an old phase with a replacement lock token.
pub fn write_status_for_session(
    session_id: &str,
    revision: u64,
    state: SessionState,
    last: Option<&str>,
    error: Option<&str>,
    last_history_id: Option<&str>,
) -> Result<(), String> {
    write_status_for_session_at(
        &status_path(),
        (session_id, revision),
        state,
        last,
        error,
        last_history_id,
    )
}

pub(crate) fn write_status_for_session_at(
    path: &Path,
    identity: (&str, u64),
    state: SessionState,
    last: Option<&str>,
    error: Option<&str>,
    last_history_id: Option<&str>,
) -> Result<(), String> {
    write_atomic_private(
        path,
        render_for_session(identity.0, identity.1, state, last, error, last_history_id).as_bytes(),
    )
}

pub fn write_recording(limit: RecordingLimit) -> Result<(), String> {
    write_atomic_private(&status_path(), render_recording(limit).as_bytes())
}

pub fn write_recording_for_session(
    session_id: &str,
    revision: u64,
    limit: RecordingLimit,
) -> Result<(), String> {
    write_recording_for_session_at(&status_path(), session_id, revision, limit)
}

pub(crate) fn write_recording_for_session_at(
    path: &Path,
    session_id: &str,
    revision: u64,
    limit: RecordingLimit,
) -> Result<(), String> {
    let mut body = render_writer("Recording");
    body.push_str(&format!("session_id={session_id}\n"));
    body.push_str(&format!("session_revision={revision}\n"));
    body.push_str(&format!("recording_limit_seconds={}\n", limit.seconds()));
    write_atomic_private(path, body.as_bytes())
}

fn render_for_session(
    session_id: &str,
    revision: u64,
    state: SessionState,
    last: Option<&str>,
    error: Option<&str>,
    last_history_id: Option<&str>,
) -> String {
    let mut body = render(state, last, error, last_history_id);
    body.push_str(&format!("session_id={session_id}\n"));
    body.push_str(&format!("session_revision={revision}\n"));
    body
}

fn render_recording(limit: RecordingLimit) -> String {
    let mut body = render_writer("Recording");
    body.push_str(&format!("recording_limit_seconds={}\n", limit.seconds()));
    body
}

pub(crate) fn render(
    state: SessionState,
    last: Option<&str>,
    error: Option<&str>,
    last_history_id: Option<&str>,
) -> String {
    let mut body = render_writer(&state_name(state));
    if let Some(text) = last {
        // The file is line-oriented; collapse newlines so a multiline
        // transcript cannot leave stray lines behind the last= field.
        let text = text.replace(['\r', '\n'], " ");
        body.push_str("last=");
        body.push_str(text.trim());
        body.push('\n');
    }
    if let Some(detail) = error {
        let detail = detail.replace(['\r', '\n'], " ");
        body.push_str("error=");
        body.push_str(detail.trim());
        body.push('\n');
    }
    if let Some(id) = last_history_id {
        body.push_str("last_history_id=");
        body.push_str(id.trim());
        body.push('\n');
    }
    body
}

fn render_writer(state: &str) -> String {
    let mut body = format!("state={state}\npid={}\n", std::process::id());
    if let Some(process) = process_identity::current() {
        body.push_str(&format!("pid_start_ticks={}\n", process.start_time_ticks));
    }
    body
}

/// Read the status file. A missing file, or an active state whose writing
/// process is no longer alive, reads as Idle. Failed states persist until the
/// next session overwrites them.
#[must_use]
pub fn read() -> Status {
    read_from(&status_path())
}

pub(crate) fn read_from(path: &Path) -> Status {
    let Some(parent) = path.parent() else {
        return Status::idle();
    };
    let status = PrivateDir::open(parent)
        .and_then(|directory| directory.read_to_string(path.file_name().unwrap_or_default()))
        .map(|raw| parse(&raw, process_identity::alive))
        .unwrap_or_else(|_| Status::idle());
    if status.state != "Idle"
        && !status.state.starts_with("Failed")
        && status
            .session_id
            .as_deref()
            .is_some_and(|id| !crate::rec::session_matches_at(parent, id))
    {
        return Status {
            state: "Idle".to_string(),
            session_id: None,
            revision: 0,
            recording_limit: None,
            ..status
        };
    }
    status
}

#[must_use]
pub fn summary() -> String {
    format!("Echo: {}", read().state)
}

fn shortcut_activation_path() -> PathBuf {
    echo_core::data_dir().join("shortcut-activation")
}

/// Return the opaque token written by the last fixed shortcut action.
#[must_use]
pub fn shortcut_activation() -> Option<String> {
    let path = shortcut_activation_path();
    let name = path.file_name()?;
    PrivateDir::open(path.parent()?)
        .and_then(|directory| directory.read_to_string(name))
        .ok()
        .filter(|token| !token.trim().is_empty())
}

/// Mark a successful action from the fixed shortcut source. GUI and tray
/// recording paths deliberately never call this.
pub fn mark_shortcut_activation(source: &str, recording_token: Option<&str>) -> Result<(), String> {
    write_shortcut_activation(&shortcut_activation_path(), source, recording_token)
}

fn write_shortcut_activation(
    path: &Path,
    source: &str,
    recording_token: Option<&str>,
) -> Result<(), String> {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let mut token = format!(
        "{source}:{}:{}:{}:{}",
        now.as_secs(),
        now.subsec_nanos(),
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    );
    if let Some(recording_token) = recording_token {
        token.push_str(":recording=");
        token.push_str(recording_token);
    }
    write_atomic_private(path, token.as_bytes())
}

#[must_use]
pub fn shortcut_recording_token(activation: &str) -> Option<&str> {
    activation
        .rsplit_once(":recording=")
        .map(|(_, token)| token.trim())
        .filter(|token| !token.is_empty())
}

fn parse(raw: &str, alive: impl Fn(ProcessIdentity) -> bool) -> Status {
    let field = |key: &str| raw.lines().find_map(|line| line.strip_prefix(key));
    let state = field("state=").unwrap_or("Idle").to_string();
    let last = field("last=")
        .filter(|text| !text.trim().is_empty())
        .map(str::to_string);
    let error = field("error=")
        .filter(|text| !text.trim().is_empty())
        .map(str::to_string);
    let last_history_id = field("last_history_id=")
        .filter(|id| !id.trim().is_empty())
        .map(str::to_string);
    let recording_limit = (state == "Recording")
        .then(|| {
            field("recording_limit_seconds=")
                .and_then(|value| value.parse::<u32>().ok())
                .and_then(RecordingLimit::new)
        })
        .flatten();
    let session_id = field("session_id=")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let revision = field("session_revision=")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let active = state != "Idle" && !state.starts_with("Failed");
    let writer = field("pid=")
        .and_then(|pid| pid.parse().ok())
        .zip(field("pid_start_ticks=").and_then(|ticks| ticks.parse().ok()))
        .map(|(pid, start_time_ticks)| ProcessIdentity {
            pid,
            start_time_ticks,
        });
    if active && !writer.is_some_and(&alive) {
        return Status {
            state: "Idle".to_string(),
            last,
            last_history_id,
            error,
            recording_limit: None,
            session_id: None,
            revision: 0,
        };
    }
    Status {
        state,
        last,
        last_history_id,
        error,
        recording_limit,
        session_id,
        revision,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn scoped_active_status_requires_the_same_live_lease() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("status");
        let lock = dir.path().join("recording.lock");
        let process = process_identity::current().unwrap();
        let owner = |token: &str, pid: u32| {
            fs::write(
                &lock,
                format!(
                    "{pid}\n{token}\n{}\nscoped-intents-v1",
                    process.start_time_ticks
                ),
            )
            .unwrap();
        };
        write_recording_for_session_at(&path, "session-a", 2, RecordingLimit::DEFAULT).unwrap();
        owner("session-a", process.pid);
        assert_eq!(read_from(&path).state, "Recording");

        owner("session-b", process.pid);
        let replaced = read_from(&path);
        assert_eq!(replaced.state, "Idle");
        assert_eq!(replaced.session_id, None);

        owner("session-a", u32::MAX);
        assert_eq!(read_from(&path).state, "Idle");
        fs::remove_file(lock).unwrap();
        assert_eq!(read_from(&path).state, "Idle");

        write_status_for_session_at(
            &path,
            ("session-a", 4),
            SessionState::Failed {
                reason: echo_core::FailReason::EngineError,
            },
            Some("saved text"),
            Some("engine failed"),
            None,
        )
        .unwrap();
        let failed = read_from(&path);
        assert!(failed.state.starts_with("Failed"));
        assert_eq!(failed.last.as_deref(), Some("saved text"));
    }

    #[test]
    fn legacy_active_status_keeps_process_identity_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("status");
        fs::write(&path, render_recording(RecordingLimit::DEFAULT)).unwrap();
        let observed = read_from(&path);
        assert_eq!(observed.state, "Recording");
        assert_eq!(observed.session_id, None);
    }

    #[test]
    fn live_recording_is_reported() {
        let status = parse("state=Recording\npid=42\npid_start_ticks=7\n", |_| true);
        assert_eq!(status.state, "Recording");
    }

    #[test]
    fn recording_limit_round_trips_for_a_live_writer() {
        let limit = echo_core::RecordingLimit::MAX;
        let body = render_recording(limit);
        let status = parse(&body, |_| true);
        assert_eq!(status.state, "Recording");
        assert_eq!(status.recording_limit, Some(limit));
        assert!(body.contains("recording_limit_seconds=600\n"));
    }

    #[test]
    fn owner_status_keeps_the_session_identity_with_its_phase() {
        let body = render_for_session("session-a", 7, SessionState::Transcribing, None, None, None);
        let status = parse(&body, |_| true);
        assert_eq!(status.state, "Transcribing");
        assert_eq!(status.session_id.as_deref(), Some("session-a"));
        assert_eq!(status.revision, 7);
    }

    #[test]
    fn old_and_malformed_recording_limits_are_ignored() {
        let old = parse("state=Recording\npid=42\n", |_| true);
        assert_eq!(old.state, "Idle");
        assert_eq!(old.recording_limit, None);

        for raw in ["0", "601", "invalid", "4294967295"] {
            let status = parse(
                &format!(
                    "state=Recording\npid=42\npid_start_ticks=7\nrecording_limit_seconds={raw}\n"
                ),
                |_| true,
            );
            assert_eq!(status.recording_limit, None);
        }
    }

    #[test]
    fn recording_with_dead_writer_reads_idle() {
        let status = parse(
            "state=Recording\npid=42\nrecording_limit_seconds=600\n",
            |_| false,
        );
        assert_eq!(status.state, "Idle");
        assert_eq!(status.recording_limit, None);
    }

    #[test]
    fn active_state_without_pid_reads_idle() {
        let status = parse("state=Transcribing\n", |_| true);
        assert_eq!(status.state, "Idle");
    }

    #[test]
    fn failed_state_persists_without_a_live_writer() {
        let status = parse("state=Failed insert was not confirmed\npid=42\n", |_| false);
        assert_eq!(status.state, "Failed insert was not confirmed");
    }

    #[test]
    fn idle_with_last_transcript() {
        let status = parse("state=Idle\npid=42\nlast=hello there\n", |_| false);
        assert_eq!(status.state, "Idle");
        assert_eq!(status.last.as_deref(), Some("hello there"));
    }

    #[test]
    fn multiline_transcript_stays_on_the_last_line() {
        let body = render(
            SessionState::Idle,
            Some("first line.\r\nsecond line."),
            None,
            None,
        );
        let status = parse(&body, |_| false);
        assert_eq!(status.state, "Idle");
        assert_eq!(status.last.as_deref(), Some("first line.  second line."));
        // No unparsed stray lines besides state, pid, and last.
        assert_eq!(body.lines().count(), 4);
    }

    #[test]
    fn engine_error_detail_survives_a_failed_state() {
        let body = render(
            SessionState::Failed {
                reason: echo_core::FailReason::EngineError,
            },
            None,
            Some("whisper-cli: failed to load model\nggml_init failed"),
            None,
        );
        let status = parse(&body, |_| false);
        assert_eq!(status.state, "Failed speech engine failed");
        assert_eq!(
            status.error.as_deref(),
            Some("whisper-cli: failed to load model ggml_init failed")
        );
    }

    #[test]
    fn persistence_error_can_coexist_with_a_recoverable_transcript() {
        let body = render(
            SessionState::Idle,
            Some("recoverable transcript"),
            Some("Transcript was not saved to history. Check data directory permissions."),
            Some("history-42"),
        );
        let status = parse(&body, |_| false);
        assert_eq!(status.state, "Idle");
        assert_eq!(status.last.as_deref(), Some("recoverable transcript"));
        assert_eq!(status.last_history_id.as_deref(), Some("history-42"));
        assert_eq!(
            status.error.as_deref(),
            Some("Transcript was not saved to history. Check data directory permissions.")
        );
    }

    #[test]
    fn reused_pid_does_not_keep_an_active_status_alive() {
        let raw = "state=Transcribing\npid=42\npid_start_ticks=7\n";
        let status = parse(raw, |writer| writer.start_time_ticks == 8);
        assert_eq!(status.state, "Idle");
    }

    #[test]
    fn shortcut_activation_tokens_are_explicit_and_monotonic() {
        let dir = std::env::temp_dir().join(format!("echo-shortcut-token-{}", std::process::id()));
        let path = dir.join("activation");
        write_shortcut_activation(&path, "native-toggle", Some("session-a")).unwrap();
        let first = fs::read_to_string(&path).unwrap();
        write_shortcut_activation(&path, "native-toggle", Some("session-b")).unwrap();
        let second = fs::read_to_string(&path).unwrap();
        assert!(first.starts_with("native-toggle:"));
        assert_eq!(shortcut_recording_token(&first), Some("session-a"));
        assert_eq!(shortcut_recording_token(&second), Some("session-b"));
        assert_ne!(first, second);
        let _ = fs::remove_dir_all(dir);
    }
}
