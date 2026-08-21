use std::env;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use echo::audio::AudioCapture;
use echo::inject::{Pasteboard, SysClipboard};
use echo_core::{DictEntry, Dictionary, History};
use serde::Serialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Manager, WindowEvent};

/// Keyboard binding the README recommends. Echo never registers it; the
/// desktop environment owns global shortcuts.
const SUGGESTED_SHORTCUT: &str = "Super+Alt+Space";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppStatus {
    phase: String,
    last_transcript: Option<String>,
    recording: bool,
    microphone_ready: bool,
    engine_name: String,
    engine_ready: bool,
    injection_name: String,
    injection_ready: bool,
    shortcut: String,
    cleanup_name: String,
    hud_enabled: bool,
    max_record_seconds: u64,
}

#[derive(Debug, Clone)]
struct Health {
    microphone_ready: bool,
    engine_name: String,
    engine_ready: bool,
    injection_name: String,
    injection_ready: bool,
}

/// Microphone, engine, and injection probes open devices and scan PATH, too
/// costly for the frontend's 400 ms status poll. Cache them briefly.
fn health_snapshot() -> Health {
    static CACHE: Mutex<Option<(Instant, Health)>> = Mutex::new(None);
    const TTL: Duration = Duration::from_secs(10);
    let mut cache = CACHE.lock().expect("health cache lock");
    if let Some((at, health)) = cache.as_ref() {
        if at.elapsed() < TTL {
            return health.clone();
        }
    }
    let (engine_name, engine_ready) = echo::stt::engine_summary();
    let (injection_name, injection_ready) = echo::inject::detection_summary();
    let health = Health {
        microphone_ready: AudioCapture::open_default().is_ok(),
        engine_name,
        engine_ready,
        injection_name,
        injection_ready,
    };
    *cache = Some((Instant::now(), health.clone()));
    health
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryItem {
    id: String,
    text: String,
    raw: String,
    engine: String,
    started_at: u64,
    infer_ms: u64,
    injection: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DictionaryItem {
    spoken: String,
    written: String,
    created_at: u64,
}

impl From<&DictEntry> for DictionaryItem {
    fn from(entry: &DictEntry) -> Self {
        Self {
            spoken: entry.spoken.clone(),
            written: entry.written.clone(),
            created_at: entry.created_at,
        }
    }
}

#[tauri::command]
fn get_app_status() -> AppStatus {
    let status = echo::status::read();
    let health = health_snapshot();
    AppStatus {
        recording: status.state == "Recording",
        phase: status.state,
        last_transcript: status.last,
        microphone_ready: health.microphone_ready,
        engine_name: health.engine_name,
        engine_ready: health.engine_ready,
        injection_name: health.injection_name,
        injection_ready: health.injection_ready,
        shortcut: SUGGESTED_SHORTCUT.to_string(),
        cleanup_name: echo::cleanup::mode_name(),
        hud_enabled: echo::ui::hud::enabled(),
        max_record_seconds: echo::rec::MAX_RECORD_SECONDS,
    }
}

#[tauri::command]
fn get_history() -> Result<Vec<HistoryItem>, String> {
    let history = History::load()?;
    Ok(history
        .rows()
        .iter()
        .rev()
        .filter(|row| !row.text.trim().is_empty())
        .map(|row| HistoryItem {
            id: row.id.clone(),
            text: row.text.clone(),
            raw: row.raw.clone(),
            engine: row.engine.to_string(),
            started_at: row.started_at,
            infer_ms: row.infer_ms,
            injection: format!("{:?}", row.inject),
        })
        .collect())
}

#[tauri::command]
fn get_dictionary() -> Result<Vec<DictionaryItem>, String> {
    let dictionary = Dictionary::load()?;
    Ok(dictionary
        .entries()
        .iter()
        .map(DictionaryItem::from)
        .collect())
}

#[tauri::command]
fn add_dictionary_entry(spoken: String, written: String) -> Result<DictionaryItem, String> {
    let spoken = spoken.trim().to_string();
    let written = written.trim().to_string();
    if spoken.is_empty() || written.is_empty() {
        return Err("Both spoken and written forms are required.".to_string());
    }
    let mut dictionary = Dictionary::load()?;
    dictionary
        .add(spoken, written)
        .map(|entry| DictionaryItem::from(&entry))
}

#[tauri::command]
fn remove_dictionary_entry(spoken: String, written: String) -> Result<bool, String> {
    Dictionary::load()?.remove(&spoken, &written)
}

#[tauri::command]
fn toggle_recording() -> Result<(), String> {
    start_recording_thread()
}

#[tauri::command]
fn copy_text(text: String) -> Result<(), String> {
    SysClipboard.set(&text)
}

fn start_recording_thread() -> Result<(), String> {
    std::thread::Builder::new()
        .name("echo-record-toggle".to_string())
        .spawn(|| {
            let _ = echo::rec::run_rec_toggle();
        })
        .map(|_| ())
        .map_err(|err| err.to_string())
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if let Some(code) = try_cli(&args) {
        std::process::exit(code);
    }
    run_desktop();
}

/// Compositor shortcuts and hold-to-talk run these subcommands without
/// starting the webview.
fn try_cli(args: &[String]) -> Option<i32> {
    match args.first().map(String::as_str) {
        None => None,
        Some("rec") => Some(rec(args.get(1..).unwrap_or(&[]))),
        Some("--hud-demo") => Some(match echo::ui::hud::run_hud_demo() {
            Ok(()) => 0,
            Err(err) => {
                eprintln!("hud-demo: {err}");
                1
            }
        }),
        Some("--help" | "-h") => {
            print_cli_usage();
            Some(0)
        }
        Some(other) => {
            eprintln!("unknown command: {other}");
            print_cli_usage();
            Some(2)
        }
    }
}

fn rec(args: &[String]) -> i32 {
    match args {
        [arg] if arg == "--once" => echo::rec::run_rec_once(),
        [arg] if arg == "--toggle" => echo::rec::run_rec_toggle(),
        [arg] if arg == "--hold" => echo::rec::run_rec_hold(),
        _ => {
            eprintln!("usage: echo-desktop rec --once|--toggle|--hold");
            2
        }
    }
}

fn print_cli_usage() {
    eprintln!("usage: echo-desktop");
    eprintln!("       echo-desktop rec --once");
    eprintln!("       echo-desktop rec --toggle");
    eprintln!("       echo-desktop rec --hold");
    eprintln!("       echo-desktop --hud-demo");
}

fn run_desktop() {
    tauri::Builder::default()
        .setup(|app| {
            let open = MenuItem::with_id(app, "open", "Open Echo", true, None::<&str>)?;
            let record =
                MenuItem::with_id(app, "record", "Start / stop recording", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &record, &quit])?;
            let mut tray = TrayIconBuilder::new().menu(&menu).tooltip("Echo");
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.on_menu_event(|app, event| match event.id().as_ref() {
                "open" => show_main_window(app),
                "record" => {
                    let _ = start_recording_thread();
                }
                "quit" => app.exit(0),
                _ => {}
            })
            .build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_app_status,
            get_history,
            get_dictionary,
            add_dictionary_entry,
            remove_dictionary_entry,
            toggle_recording,
            copy_text,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Echo");
}
