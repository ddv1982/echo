use std::env;
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};

use commands::{
    add_dictionary_entries_batch, add_dictionary_entry, cancel_dictionary_training_sample,
    clear_history, copy_text, delete_history_item, finish_dictionary_training_sample,
    get_app_status, get_dictionary, get_history, get_microphones, get_recording_level,
    get_settings, get_shortcut_status, list_gpu_devices, list_languages, list_models, quit_app,
    remove_dictionary_entry, remove_stale_installs, repair_legacy_shortcut, retry_shortcut,
    set_microphone, set_settings, start_dictionary_training_sample, stop_recording,
    test_input_device, test_microphone_fallback, toggle_recording, DictionaryTrainingCaptures,
};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Manager, WindowEvent};

mod blocking;
mod cli;
mod commands;
#[cfg(feature = "status-perf-probe")]
mod perf;
mod settings;
mod setup;
mod shortcuts;
mod speech;
mod status;

#[cfg(test)]
use status::{Health, HEALTH};

const APP_ID: &str = "io.github.ddv1982.echo";

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        #[cfg(target_os = "linux")]
        let was_hidden = matches!(window.is_visible(), Ok(false));
        let _ = window.show();
        let _ = window.unminimize();
        #[cfg(target_os = "linux")]
        if was_hidden
            && env::var_os("WAYLAND_DISPLAY")
                .map(|display| !display.is_empty())
                .unwrap_or(false)
            && !env::var_os("GDK_BACKEND")
                .map(|backends| {
                    backends
                        .to_string_lossy()
                        .split(',')
                        .any(|backend| backend.trim().eq_ignore_ascii_case("x11"))
                })
                .unwrap_or(false)
        {
            // Remove with the Tauri 2.12/tao 0.36 Wayland decoration fix.
            if let Err(err) = window.set_resizable(false) {
                eprintln!("window workaround: failed to disable resizability: {err}");
            } else if let Err(err) = window.set_resizable(true) {
                eprintln!("window workaround: failed to restore resizability: {err}; retrying");
                if let Err(retry_err) = window.set_resizable(true) {
                    eprintln!("window workaround: resizability restore retry failed: {retry_err}");
                }
            }
        }
        let _ = window.set_focus();
    }
}

fn main() {
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    if let Some(result) = echo::upgrade::run_restart_helper(&args) {
        if let Err(error) = result {
            eprintln!("echo-desktop: replacement handoff failed: {error}");
            std::process::exit(1);
        }
        run_desktop();
        return;
    }
    if args.is_empty() {
        run_desktop();
    } else {
        std::process::exit(cli::run(args));
    }
}

#[cfg(not(feature = "status-perf-probe"))]
struct UpgradeWatch {
    path: std::path::PathBuf,
    identity: echo::upgrade::FileIdentity,
}

fn run_desktop() {
    let mut context = tauri::generate_context!();
    context.config_mut().app.tray_icon = None;
    let builder = tauri::Builder::default()
        .manage(setup::SetupService::default())
        .manage(DictionaryTrainingCaptures::default());
    #[cfg(not(feature = "status-perf-probe"))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
        let Some(watch) = app.try_state::<UpgradeWatch>() else {
            show_main_window(app);
            return;
        };
        let current = echo::upgrade::file_identity(&watch.path).ok();
        match echo::upgrade::second_launch_decision(
            watch.identity,
            current,
            echo::rec::session_active(),
        ) {
            echo::upgrade::SecondLaunch::Focus => {
                eprintln!("echo-desktop: second launch; focusing the running window");
                show_main_window(app);
            }
            echo::upgrade::SecondLaunch::DeferRestart => {
                eprintln!(
                    "echo-desktop: binary changed on disk; deferring restart while recording"
                );
                show_main_window(app);
            }
            echo::upgrade::SecondLaunch::Restart => {
                match echo::rec::attempt_upgrade_takeover(|| {
                    let args = echo::upgrade::restart_helper_args().map_err(std::io::Error::other)?;
                    std::process::Command::new(&watch.path).args(args).spawn().map(|_| ())
                }) {
                    echo::rec::UpgradeTakeover::Spawned => {
                        eprintln!(
                            "echo-desktop: binary changed on disk; restarting into the new build"
                        );
                        app.exit(0);
                    }
                    echo::rec::UpgradeTakeover::Deferred => {
                        eprintln!(
                            "echo-desktop: binary changed on disk; deferring restart while recording"
                        );
                        show_main_window(app);
                    }
                    echo::rec::UpgradeTakeover::SpawnFailed(err) => {
                        eprintln!("echo-desktop: restart spawn failed: {err}");
                        show_main_window(app);
                    }
                }
            }
        }
    }));
    let result = builder
        .setup(|app| {
            #[cfg(not(feature = "status-perf-probe"))]
            match echo::upgrade::terminate_old_echo_processes() {
                echo::upgrade::StartupCleanup::Defer => {
                    eprintln!("echo-desktop: deferring startup takeover while recording is active");
                }
                echo::upgrade::StartupCleanup::TerminateStaleGui => {}
            }
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
            #[cfg(not(feature = "status-perf-probe"))]
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
            delete_history_item,
            clear_history,
            get_dictionary,
            add_dictionary_entry,
            add_dictionary_entries_batch,
            remove_dictionary_entry,
            start_dictionary_training_sample,
            finish_dictionary_training_sample,
            cancel_dictionary_training_sample,
            toggle_recording,
            stop_recording,
            get_recording_level,
            copy_text,
            quit_app,
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
            #[cfg(feature = "status-perf-probe")]
            perf::perf_noop,
            #[cfg(feature = "status-perf-probe")]
            perf::perf_fixed_status,
            #[cfg(feature = "status-perf-probe")]
            perf::perf_clear_status_stages,
            #[cfg(feature = "status-perf-probe")]
            perf::perf_preserve_cold_status_stage,
            #[cfg(feature = "status-perf-probe")]
            perf::perf_report_complete,
            #[cfg(feature = "status-perf-probe")]
            perf::perf_report_failed,
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
