use echo_desktop::ipc::RecordingSnapshot;

fn stop_reply(ack: Option<echo::rec::RecordingControlAck>) -> Result<RecordingSnapshot, String> {
    let ack =
        ack.ok_or_else(|| "recording session changed before stop was accepted".to_string())?;
    Ok(RecordingSnapshot {
        session_id: Some(ack.session_id),
        phase: echo_desktop::ipc::AppPhase::Recording,
        capture_stop_requested: true,
        revision: ack.revision,
    })
}

fn cancel_reply(ack: Option<echo::rec::RecordingControlAck>) -> Result<RecordingSnapshot, String> {
    let ack = ack
        .ok_or_else(|| "recording session changed before cancellation was accepted".to_string())?;
    Ok(RecordingSnapshot {
        session_id: Some(ack.session_id),
        phase: echo_desktop::ipc::AppPhase::Transcribing,
        capture_stop_requested: false,
        revision: ack.revision,
    })
}

/// Explicit GUI start. The recording owner publishes status; this command
/// only starts that owner and returns its identity acknowledgement.
#[tauri::command]
pub(crate) async fn start_capture() -> Result<RecordingSnapshot, String> {
    crate::blocking::run_blocking("start recording", || {
        let started = echo::rec::start_managed_recording()?;
        Ok(RecordingSnapshot {
            session_id: Some(started.session_id),
            phase: echo_desktop::ipc::AppPhase::Recording,
            capture_stop_requested: false,
            revision: started.revision,
        })
    })
    .await?
}

#[tauri::command]
pub(crate) async fn stop_capture(session_id: String) -> Result<RecordingSnapshot, String> {
    crate::blocking::run_blocking("stop capture", move || {
        stop_reply(echo::rec::request_capture_stop_ack(&session_id)?)
    })
    .await?
}

#[tauri::command]
pub(crate) async fn cancel_transcription(session_id: String) -> Result<RecordingSnapshot, String> {
    crate::blocking::run_blocking("cancel transcription", move || {
        cancel_reply(echo::rec::request_transcription_cancel_ack(&session_id)?)
    })
    .await?
}

#[tauri::command]
pub(crate) fn stop_recording(activation: String) -> Result<bool, String> {
    echo::rec::stop_shortcut_recording(&activation)
}

#[tauri::command]
pub(crate) fn get_recording_level() -> f32 {
    if echo::rec::recording_in_process() {
        echo::audio::process_meter().level()
    } else {
        0.0
    }
}

/// Tray and shortcut retain their public toggle affordance. Desktop Home uses
/// the explicit session-bound commands above.
pub(crate) fn start_recording_thread() -> Result<Option<String>, String> {
    echo::rec::toggle_managed_recording()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_controls_cannot_reply_with_a_replacement_snapshot() {
        assert!(stop_reply(None).unwrap_err().contains("session changed"));
        assert!(cancel_reply(None).unwrap_err().contains("session changed"));
    }
}
