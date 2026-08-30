use echo_desktop::ipc::{LegacyShortcutSetup, ShortcutStatus};

#[tauri::command]
pub(crate) fn get_shortcut_status() -> ShortcutStatus {
    crate::shortcuts::status(&crate::status::current_exe_string())
}

#[tauri::command]
pub(crate) fn retry_shortcut() -> ShortcutStatus {
    crate::shortcuts::retry()
}

#[tauri::command]
pub(crate) fn repair_legacy_shortcut() -> Result<LegacyShortcutSetup, String> {
    crate::shortcuts::repair(&crate::status::current_exe_string())
}
