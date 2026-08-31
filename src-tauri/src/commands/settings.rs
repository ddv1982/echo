use echo_desktop::ipc::{SettingsChange, SettingsSnapshot};
use tauri::State;

#[tauri::command]
pub(crate) async fn get_settings(
    state: State<'_, crate::setup::SetupService>,
) -> Result<SettingsSnapshot, String> {
    let service = state.inner().clone();
    crate::blocking::run_blocking("settings snapshot", move || {
        crate::settings::snapshot(service.snapshot())
    })
    .await?
}

#[tauri::command]
pub(crate) async fn set_settings(
    change: SettingsChange,
    state: State<'_, crate::setup::SetupService>,
) -> Result<SettingsSnapshot, String> {
    let service = state.inner().clone();
    crate::blocking::run_blocking("settings change", move || {
        crate::settings::change(change)?;
        crate::settings::snapshot(service.snapshot())
    })
    .await?
}
