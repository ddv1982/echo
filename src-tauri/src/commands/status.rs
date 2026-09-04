use echo_desktop::ipc::AppStatus;

#[tauri::command]
pub(crate) async fn get_app_status() -> Result<AppStatus, String> {
    crate::blocking::run_blocking("application status", crate::status::app_status).await
}
