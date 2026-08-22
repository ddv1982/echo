use std::fs;
use std::path::Path;

use echo_core::{status_path, write_atomic, SessionState};

/// Status file contents after staleness handling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub state: String,
    pub last: Option<String>,
    /// Engine stderr from a failed session, so the desktop app can show what
    /// the engine actually said instead of "speech engine failed".
    pub error: Option<String>,
}

impl Status {
    #[must_use]
    pub fn idle() -> Self {
        Self {
            state: "Idle".to_string(),
            last: None,
            error: None,
        }
    }
}

#[must_use]
pub fn state_name(state: SessionState) -> String {
    match state {
        SessionState::Idle => "Idle".to_string(),
        SessionState::Recording { .. } => "Recording".to_string(),
        SessionState::Transcribing => "Transcribing".to_string(),
        SessionState::Cleaning => "Cleaning".to_string(),
        SessionState::Injecting => "Injecting".to_string(),
        SessionState::Failed { reason } => format!("Failed {}", reason.as_str()),
    }
}

/// Persist the session state for other processes. The file carries the
/// writer's pid so readers can spot a session whose process died.
pub fn write_status(state: SessionState, last: Option<&str>, error: Option<&str>) -> Result<(), String> {
    write_atomic(&status_path(), render(state, last, error).as_bytes())
}

fn render(state: SessionState, last: Option<&str>, error: Option<&str>) -> String {
    let mut body = format!("state={}\npid={}\n", state_name(state), std::process::id());
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
    body
}

/// Read the status file. A missing file, or an active state whose writing
/// process is no longer alive, reads as Idle. Failed states persist until the
/// next session overwrites them.
#[must_use]
pub fn read() -> Status {
    match fs::read_to_string(status_path()) {
        Ok(raw) => parse(&raw, pid_alive),
        Err(_) => Status::idle(),
    }
}

#[must_use]
pub fn summary() -> String {
    format!("Echo: {}", read().state)
}

fn parse(raw: &str, alive: impl Fn(&str) -> bool) -> Status {
    let field = |key: &str| raw.lines().find_map(|line| line.strip_prefix(key));
    let state = field("state=").unwrap_or("Idle").to_string();
    let last = field("last=")
        .filter(|text| !text.trim().is_empty())
        .map(str::to_string);
    let error = field("error=")
        .filter(|text| !text.trim().is_empty())
        .map(str::to_string);
    let active = state != "Idle" && !state.starts_with("Failed");
    if active && !field("pid=").map(alive).unwrap_or(false) {
        return Status {
            state: "Idle".to_string(),
            last,
            error,
        };
    }
    Status { state, last, error }
}

fn pid_alive(pid: &str) -> bool {
    pid.trim()
        .parse::<u32>()
        .map(|pid| Path::new("/proc").join(pid.to_string()).exists())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_recording_is_reported() {
        let status = parse("state=Recording\npid=42\n", |_| true);
        assert_eq!(status.state, "Recording");
    }

    #[test]
    fn recording_with_dead_writer_reads_idle() {
        let status = parse("state=Recording\npid=42\n", |_| false);
        assert_eq!(status.state, "Idle");
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
        let body = render(SessionState::Idle, Some("first line.\r\nsecond line."), None);
        let status = parse(&body, |_| false);
        assert_eq!(status.state, "Idle");
        assert_eq!(status.last.as_deref(), Some("first line.  second line."));
        // No unparsed stray lines besides state, pid, and last.
        assert_eq!(body.lines().count(), 3);
    }

    #[test]
    fn engine_error_detail_survives_a_failed_state() {
        let body = render(
            SessionState::Failed {
                reason: echo_core::FailReason::EngineError,
            },
            None,
            Some("whisper-cli: failed to load model\nggml_init failed"),
        );
        let status = parse(&body, |_| false);
        assert_eq!(status.state, "Failed speech engine failed");
        assert_eq!(
            status.error.as_deref(),
            Some("whisper-cli: failed to load model ggml_init failed")
        );
    }
}
