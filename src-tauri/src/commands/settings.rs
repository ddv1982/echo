use echo_desktop::ipc::{SettingsChange, SettingsSnapshot};
use tauri::{AppHandle, State};

#[cfg(feature = "status-perf-probe")]
use tauri::Manager;

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
    app: AppHandle,
) -> Result<SettingsSnapshot, String> {
    let service = state.inner().clone();
    apply_settings_change(change, service, app).await
}

async fn apply_settings_change(
    change: SettingsChange,
    service: crate::setup::SetupService,
    app: AppHandle,
) -> Result<SettingsSnapshot, String> {
    let tray_request = crate::tray::request();
    let (revision, snapshot) = crate::blocking::run_blocking("settings change", move || {
        crate::settings::change(change)?;
        crate::settings::snapshot_with_revision(|| service.snapshot())
    })
    .await??;
    crate::tray::sync(&app, tray_request, revision, &snapshot);
    Ok(snapshot)
}

#[cfg(feature = "status-perf-probe")]
pub(crate) fn run_test_hook(app: &AppHandle) {
    let Ok(value) = std::env::var("ECHO_TRAY_TEST_SETTINGS_LANGUAGE") else {
        return;
    };
    let service = app.state::<crate::setup::SetupService>().inner().clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = apply_settings_change(
            SettingsChange::Language { value: Some(value) },
            service,
            app,
        )
        .await
        {
            eprintln!("tray settings test hook: {error}");
        }
    });
}
