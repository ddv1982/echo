use echo_desktop::ipc::{LegacyShortcutSetup, ShortcutStatus};

#[tauri::command]
pub(crate) fn get_shortcut_status() -> ShortcutStatus {
    crate::shortcuts::status(&crate::status::current_exe_string())
}

#[tauri::command]
pub(crate) async fn retry_shortcut() -> Result<ShortcutStatus, String> {
    crate::blocking::run_blocking("shortcut retry", crate::shortcuts::retry).await
}

#[tauri::command]
pub(crate) async fn repair_legacy_shortcut() -> Result<LegacyShortcutSetup, String> {
    crate::blocking::run_blocking("legacy shortcut repair", || {
        crate::shortcuts::repair(&crate::status::current_exe_string())
    })
    .await?
}
