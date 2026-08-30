use echo_desktop::ipc::Settings;

#[tauri::command]
pub(crate) fn get_settings() -> Result<Settings, String> {
    crate::settings::read()
}

#[tauri::command]
pub(crate) fn set_settings(settings: Settings) -> Result<Settings, String> {
    crate::settings::write(settings)
}
