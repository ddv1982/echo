use std::env;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};

use commands::{
    add_dictionary_entry, copy_text, get_app_status, get_dictionary, get_history, get_microphones,
    get_recording_level, get_settings, get_shortcut_status, list_gpu_devices, list_languages,
    list_models, remove_dictionary_entry, remove_stale_installs, repair_legacy_shortcut,
    retry_shortcut, set_microphone, set_settings, stop_recording, test_input_device,
    test_microphone_fallback, toggle_recording,
};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Manager, WindowEvent};

mod cli;
mod commands;
mod settings;
mod setup;
mod shortcuts;
mod status;

#[cfg(test)]
use status::{Health, HEALTH};

const APP_ID: &str = "io.github.ddv1982.echo";

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn main() {
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        run_desktop();
    } else {
        std::process::exit(cli::run(args));
    }
}

struct UpgradeWatch {
    path: std::path::PathBuf,
    identity: echo::upgrade::FileIdentity,
}

fn run_desktop() {
    let mut context = tauri::generate_context!();
    context.config_mut().app.tray_icon = None;
    let result = tauri::Builder::default()
        .manage(setup::SetupService::default())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            let Some(watch) = app.try_state::<UpgradeWatch>() else {
                show_main_window(app);
                return;
            };
            let current = echo::upgrade::file_identity(&watch.path).ok();
            match echo::upgrade::second_launch_decision(watch.identity, current) {
                echo::upgrade::SecondLaunch::Focus => {
                    eprintln!("echo-desktop: second launch; focusing the running window");
                    show_main_window(app);
                }
                echo::upgrade::SecondLaunch::Restart => {
                    match std::process::Command::new(&watch.path).spawn() {
                        Ok(_) => {
                            eprintln!("echo-desktop: binary changed on disk; restarting into the new build");
                            app.exit(0);
                        }
                        Err(err) => {
                            eprintln!("echo-desktop: restart spawn failed: {err}");
                            show_main_window(app);
                        }
                    }
                }
            }
        }))
        .setup(|app| {
            echo::upgrade::terminate_old_echo_processes();
            let open = MenuItem::with_id(app, "open", "Open Echo", true, None::<&str>)?;
            let record =
                MenuItem::with_id(app, "record", "Start / stop recording", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &record, &quit])?;
            let icon = Image::from_bytes(include_bytes!("../icons/tray-24.png"))
                .expect("tray-24.png decodes as RGBA");
            let tray = TrayIconBuilder::new()
                .menu(&menu)
                .icon(icon)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "open" => show_main_window(app),
                    "record" => {
                        let _ = commands::start_recording_thread();
                    }
                    "quit" => app.exit(0),
                    _ => {}
                });
            let tray_ready = match panic::catch_unwind(AssertUnwindSafe(|| tray.build(app))) {
                Ok(Ok(_)) => true,
                Ok(Err(err)) => {
                    eprintln!("tray icon: {err}");
                    false
                }
                Err(_) => {
                    eprintln!("tray icon: libayatana-appindicator failed to load");
                    false
                }
            };
            app.manage(AtomicBool::new(tray_ready));
            shortcuts::reconcile();
            if let Ok(path) = std::env::current_exe().and_then(|path| path.canonicalize()) {
                if let Ok(identity) = echo::upgrade::file_identity(&path) {
                    app.manage(UpgradeWatch { path, identity });
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.state::<AtomicBool>().load(Ordering::SeqCst) {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_app_status,
            get_shortcut_status,
            retry_shortcut,
            repair_legacy_shortcut,
            get_history,
            get_dictionary,
            add_dictionary_entry,
            remove_dictionary_entry,
            toggle_recording,
            stop_recording,
            get_recording_level,
            copy_text,
            remove_stale_installs,
            get_settings,
            set_settings,
            list_models,
            list_gpu_devices,
            list_languages,
            setup::get_readiness,
            setup::start_setup,
            setup::repair_managed,
            setup::verify_managed,
            setup::remove_managed,
            setup::cancel_setup,
            get_microphones,
            set_microphone,
            test_input_device,
            test_microphone_fallback,
        ])
        .run(context);
    shortcuts::shutdown();
    result.expect("error while running Echo");
}

#[cfg(test)]
fn shortcut_test_handler<R: tauri::Runtime>(
) -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static {
    tauri::generate_handler![get_app_status, repair_legacy_shortcut]
}
