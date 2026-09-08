use std::path::Path;

use echo_core::PrivateDir;

use crate::status;

use super::lease::{
    intent_path, live_lock_owner, stop_request_matches, LockOwner, ToggleAction, ToggleSession,
};

/// Decide the meaning of a toggle before writing a capture-stop signal. The
/// token check prevents an observation of an earlier owner from authorizing a
/// cancellation for a replacement owner.
pub(super) fn start_or_stop_with_intent() -> Result<(ToggleAction, bool), String> {
    decide_toggle_intent(status::read, |observed| {
        ToggleSession::start_or_stop_in(&echo_core::data_dir(), observed)
    })
}

pub(super) fn decide_toggle_intent(
    read_status: impl FnOnce() -> status::Status,
    start_or_stop: impl FnOnce(Option<&str>) -> Result<ToggleAction, String>,
) -> Result<(ToggleAction, bool), String> {
    let observed = read_status();
    let action = start_or_stop(observed.session_id.as_deref())?;
    let cancel_transcription =
        matches!(&action, ToggleAction::Stop(owner) if should_cancel_toggle(&observed, owner));
    Ok((action, cancel_transcription))
}

pub(super) fn should_cancel_toggle(observed: &status::Status, owner: &LockOwner) -> bool {
    observed.state == "Transcribing" && observed.session_id.as_deref() == owner.token.as_deref()
}

pub(super) fn finish_toggle_stop(
    owner: LockOwner,
    cancel_transcription: bool,
) -> Result<Option<String>, String> {
    finish_toggle_stop_with(owner, cancel_transcription, apply_toggle_stop_intent)
}

pub(super) fn finish_toggle_stop_with(
    owner: LockOwner,
    cancel_transcription: bool,
    apply: impl FnOnce(&LockOwner) -> Result<(), String>,
) -> Result<Option<String>, String> {
    if cancel_transcription {
        apply(&owner)?;
    }
    Ok(owner.token)
}

pub(super) fn apply_toggle_stop_intent(owner: &LockOwner) -> Result<(), String> {
    apply_toggle_stop_intent_in(&echo_core::data_dir(), owner)
}

pub(super) fn apply_toggle_stop_intent_in(dir: &Path, owner: &LockOwner) -> Result<(), String> {
    apply_toggle_stop_intent_with(owner, |token| {
        ToggleSession::request_intent_for_token_in(dir, token, ControlIntent::TranscriptionCancel)
    })
}

pub(super) fn apply_toggle_stop_intent_with(
    owner: &LockOwner,
    write_cancel: impl FnOnce(&str) -> Result<bool, String>,
) -> Result<(), String> {
    let Some(token) = owner.token.as_deref() else {
        return Err(session_changed_before_cancel());
    };
    match write_cancel(token) {
        Ok(true) => Ok(()),
        Ok(false) => Err(session_changed_before_cancel()),
        Err(err) => Err(err),
    }
}

pub(super) fn session_changed_before_cancel() -> String {
    "recording session changed before cancellation was accepted".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordingControlAck {
    pub session_id: String,
    pub revision: u64,
}

pub fn request_capture_stop(session_id: &str) -> Result<bool, String> {
    Ok(request_capture_stop_ack(session_id)?.is_some())
}

pub fn request_capture_stop_ack(session_id: &str) -> Result<Option<RecordingControlAck>, String> {
    request_control_ack(session_id, ControlIntent::CaptureStop)
}

pub fn request_transcription_cancel(session_id: &str) -> Result<bool, String> {
    Ok(request_transcription_cancel_ack(session_id)?.is_some())
}

pub fn request_transcription_cancel_ack(
    session_id: &str,
) -> Result<Option<RecordingControlAck>, String> {
    request_control_ack(session_id, ControlIntent::TranscriptionCancel)
}

#[derive(Clone, Copy)]
pub(super) enum ControlIntent {
    CaptureStop,
    TranscriptionCancel,
}

impl ControlIntent {
    pub(super) fn phase(self) -> &'static str {
        match self {
            Self::CaptureStop => "Recording",
            Self::TranscriptionCancel => "Transcribing",
        }
    }

    pub(super) fn file_kind(self) -> &'static str {
        match self {
            Self::CaptureStop => "stop",
            Self::TranscriptionCancel => "cancel",
        }
    }
}

impl RecordingControlAck {
    /// Accepted intent occupies the revision before the next owner publication.
    #[must_use]
    pub fn after_revision(owner_revision: u64) -> u64 {
        owner_revision.saturating_add(1)
    }
}

pub(super) fn request_control_ack(
    session_id: &str,
    intent: ControlIntent,
) -> Result<Option<RecordingControlAck>, String> {
    request_control_ack_with(session_id, intent, status::read, |session_id, intent| {
        ToggleSession::request_intent_for_token_in(&echo_core::data_dir(), session_id, intent)
    })
}

pub(super) fn request_control_ack_with(
    session_id: &str,
    intent: ControlIntent,
    read_status: impl Fn() -> status::Status,
    write_intent: impl FnOnce(&str, ControlIntent) -> Result<bool, String>,
) -> Result<Option<RecordingControlAck>, String> {
    let current = read_status();
    if !control_applies_to(&current, session_id, intent) {
        return Ok(None);
    }
    if !write_intent(session_id, intent)? {
        return Ok(None);
    }
    let latest = read_status();
    if !control_applies_to(&latest, session_id, intent) {
        return Ok(None);
    }
    Ok(Some(RecordingControlAck {
        session_id: session_id.to_string(),
        revision: RecordingControlAck::after_revision(latest.revision),
    }))
}

pub(super) fn control_applies_to(
    status: &status::Status,
    session_id: &str,
    intent: ControlIntent,
) -> bool {
    status.state == intent.phase() && status.session_id.as_deref() == Some(session_id)
}

#[must_use]
pub fn capture_stop_requested_for(session_id: Option<&str>) -> bool {
    let Some(session_id) = session_id else {
        return false;
    };
    let dir = echo_core::data_dir();
    let owner = live_lock_owner(&dir.join("recording.lock"));
    let Some(owner) = owner.filter(|owner| owner.token.as_deref() == Some(session_id)) else {
        return false;
    };
    let scoped = PrivateDir::open(&dir)
        .ok()
        .and_then(|directory| {
            directory
                .read_to_string(intent_path(&dir, "stop", &owner).file_name()?.as_ref())
                .ok()
        })
        .is_some_and(|request| stop_request_matches(Some(session_id), &request));
    scoped
        || PrivateDir::open(&dir)
            .ok()
            .and_then(|directory| directory.read_to_string("recording.stop".as_ref()).ok())
            .is_some_and(|request| stop_request_matches(Some(session_id), &request))
}
