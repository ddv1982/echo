mod control;
mod lease;
mod pipeline;
mod upgrade;

pub use control::{
    capture_stop_requested_for, request_capture_stop, request_capture_stop_ack,
    request_transcription_cancel, request_transcription_cancel_ack, RecordingControlAck,
};
pub use lease::{recording_in_process, session_active, RecordingSession};
pub(crate) use lease::{session_active_at, session_matches_at};
pub use pipeline::recording_limit_from_process;
pub use upgrade::{
    attempt_upgrade_takeover, reserve_upgrade_takeover, TakeoverReservation, UpgradeTakeover,
};

use control::{finish_toggle_stop, start_or_stop_with_intent, ControlIntent};
use lease::{LockAcquisition, ToggleAction, ToggleSession};
use pipeline::{run_record, run_record_started, StopWhen};

use crate::status;

pub fn run_rec_once() -> i32 {
    match RecordingSession::acquire() {
        Ok(RecordingSession(session)) => run_record(StopWhen::Timer(Some(session))),
        Err(err) => {
            eprintln!("rec: {err}");
            1
        }
    }
}

pub fn run_rec_toggle() -> i32 {
    match start_or_stop_with_intent() {
        Ok((action, cancel_transcription)) => {
            let recording_token = action.recording_token().map(str::to_string);
            if let Err(err) =
                status::mark_shortcut_activation("toggle-command", recording_token.as_deref())
            {
                eprintln!("toggle: cannot record shortcut provenance: {err}");
            }
            match action {
                ToggleAction::Start(session) => run_record(StopWhen::ToggleFile(session)),
                ToggleAction::Stop(owner) => {
                    // Preserve the CLI/hotkey toggle convention after capture:
                    // its stop gesture means cancel while transcription is live.
                    // Explicit desktop capture-stop never takes this path.
                    match finish_toggle_stop(owner, cancel_transcription) {
                        Ok(_) => 0,
                        Err(err) => {
                            eprintln!("toggle: {err}");
                            1
                        }
                    }
                }
            }
        }
        Err(err) => {
            eprintln!("toggle: {err}");
            1
        }
    }
}

/// Toggle an in-process recording after synchronously acquiring or stopping
/// the cross-process session. Recording work continues on a background thread.
pub fn toggle_managed_recording() -> Result<Option<String>, String> {
    match start_or_stop_with_intent()? {
        (ToggleAction::Start(session), _) => {
            let recording_token = session.token.clone();
            std::thread::Builder::new()
                .name("echo-record-toggle".to_string())
                .spawn(move || {
                    let _ = run_record(StopWhen::ToggleFile(session));
                })
                .map(|_| Some(recording_token))
                .map_err(|err| err.to_string())
        }
        (ToggleAction::Stop(owner), cancel_transcription) => {
            finish_toggle_stop(owner, cancel_transcription)
        }
    }
}

/// The identity and revision acknowledged by the owner when capture starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartedRecording {
    pub session_id: String,
    pub revision: u64,
}

pub fn start_managed_recording() -> Result<StartedRecording, String> {
    let session = match ToggleSession::acquire_in(&echo_core::data_dir())? {
        LockAcquisition::Started(session) => session,
        LockAcquisition::Busy(_) => return Err("Another recording is already active.".to_string()),
    };
    let token = session.token.clone();
    let (send, receive) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("echo-record-start".to_string())
        .spawn(move || {
            let _ = run_record_started(StopWhen::ToggleFile(session), Some(send));
        })
        .map_err(|err| err.to_string())?;
    let revision = receive
        .recv()
        .map_err(|_| "recording worker exited before starting".to_string())??;
    Ok(StartedRecording {
        session_id: token,
        revision,
    })
}

pub fn stop_shortcut_recording(activation: &str) -> Result<bool, String> {
    let current = status::shortcut_activation();
    if current.as_deref().map(str::trim) != Some(activation.trim()) {
        return Ok(false);
    }
    let Some(recording_token) = status::shortcut_recording_token(activation) else {
        return Ok(false);
    };
    ToggleSession::request_intent_for_token_in(
        &echo_core::data_dir(),
        recording_token,
        ControlIntent::CaptureStop,
    )
}

#[cfg(test)]
mod tests;
