use echo_desktop::ipc::{ChannelReply, SettingsChange, SettingsSnapshot};
use tauri::ipc::Channel;
use tauri::{AppHandle, State};

#[cfg(feature = "status-perf-probe")]
use tauri::Manager;

#[tauri::command]
pub(crate) fn get_settings(
    state: State<'_, crate::setup::SetupService>,
    owner: State<'_, crate::settings::ConfigMutationService>,
    reply: Channel<ChannelReply<SettingsSnapshot>>,
) -> Result<(), String> {
    let service = state.inner().clone();
    owner.request_settings_snapshot(service, reply)
}

#[tauri::command]
pub(crate) fn set_settings(
    change: SettingsChange,
    state: State<'_, crate::setup::SetupService>,
    owner: State<'_, crate::settings::ConfigMutationService>,
    app: AppHandle,
    reply: Channel<ChannelReply<SettingsSnapshot>>,
) -> Result<(), String> {
    let service = state.inner().clone();
    let tray_request = crate::tray::request();
    owner.request_settings_change(change, service, app, tray_request, reply)
}

#[cfg(feature = "status-perf-probe")]
pub(crate) fn run_test_hook(app: &AppHandle) {
    let Ok(value) = std::env::var("ECHO_TRAY_TEST_SETTINGS_LANGUAGE") else {
        return;
    };
    let service = app.state::<crate::setup::SetupService>().inner().clone();
    let Some(owner) = app.try_state::<crate::settings::ConfigMutationService>() else {
        return;
    };
    let app = app.clone();
    let tray_request = crate::tray::request();
    if let Err(error) = owner.request_settings_change(
        SettingsChange::Language { value: Some(value) },
        service,
        app,
        tray_request,
        Channel::new(|body| {
            let message: serde_json::Value = body.deserialize()?;
            if let Some(error) = message.get("error").and_then(serde_json::Value::as_str) {
                eprintln!("tray settings test hook: {error}");
            }
            Ok(())
        }),
    ) {
        eprintln!("tray settings test hook: {error}");
    }
}
