use echo_desktop::ipc::RecordingSnapshot;

fn snapshot() -> RecordingSnapshot {
    crate::status::recording_snapshot(&echo::status::read())
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
        let ack = echo::rec::request_capture_stop_ack(&session_id)?;
        Ok(match ack {
            Some(ack) => RecordingSnapshot {
                session_id: Some(ack.session_id),
                phase: echo_desktop::ipc::AppPhase::Recording,
                capture_stop_requested: true,
                revision: ack.revision,
            },
            None => snapshot(),
        })
    })
    .await?
}

#[tauri::command]
pub(crate) async fn cancel_transcription(session_id: String) -> Result<RecordingSnapshot, String> {
    crate::blocking::run_blocking("cancel transcription", move || {
        let _ = echo::rec::request_transcription_cancel(&session_id)?;
        Ok(snapshot())
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
