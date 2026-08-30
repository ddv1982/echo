#[tauri::command]
pub(crate) fn toggle_recording() -> Result<(), String> {
    start_recording_thread().map(|_| ())
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

pub(crate) fn start_recording_thread() -> Result<Option<String>, String> {
    echo::rec::toggle_managed_recording()
}
