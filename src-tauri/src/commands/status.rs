use echo_desktop::ipc::AppStatus;

#[tauri::command]
pub(crate) fn get_app_status() -> AppStatus {
    crate::status::app_status()
}
