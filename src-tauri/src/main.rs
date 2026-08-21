use std::path::PathBuf;

use echo::audio::AudioCapture;
use echo::inject::{Pasteboard, SysClipboard};
use echo_core::{DictEntry, Dictionary, History};
use serde::Serialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Manager, WindowEvent};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppStatus {
    phase: String,
    last_transcript: Option<String>,
    recording: bool,
    microphone_ready: bool,
    model_ready: bool,
    engine_name: String,
    injection_name: String,
    shortcut: String,
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
    let (phase, last_transcript) = read_status_file();
    let whisper_runner = ["whisper-cli", "whisper-cpp", "whisper"]
        .into_iter()
        .find(|name| on_path(name));
    let model = whisper_model();
    let model_ready = whisper_runner.is_some() && model.is_some();
    let engine_name = if model_ready {
        "Whisper · base.en".to_string()
    } else {
        "Whisper setup required".to_string()
    };
    let injection_name = if is_wayland() && on_path("ydotool") {
        "ydotool · Wayland".to_string()
    } else if on_path("xdotool") {
        "xdotool · X11".to_string()
    } else {
        "Clipboard fallback".to_string()
    };
    AppStatus {
        recording: phase == "Recording",
        phase,
        last_transcript,
        microphone_ready: AudioCapture::open_default().is_ok(),
        model_ready,
        engine_name,
        injection_name,
        shortcut: "Super+Alt+Space".to_string(),
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

fn read_status_file() -> (String, Option<String>) {
    let raw = std::fs::read_to_string(echo_core::status_path()).unwrap_or_default();
    let phase = raw
        .lines()
        .find_map(|line| line.strip_prefix("state="))
        .unwrap_or("Idle")
        .to_string();
    let last = raw
        .lines()
        .find_map(|line| line.strip_prefix("last="))
        .filter(|text| !text.trim().is_empty())
        .map(str::to_string);
    (phase, last)
}

fn whisper_model() -> Option<PathBuf> {
    let dir = std::env::var_os("ECHO_MODEL_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CACHE_HOME").map(|path| PathBuf::from(path).join("echo")))
        .or_else(|| {
            std::env::var_os("HOME").map(|path| PathBuf::from(path).join(".cache").join("echo"))
        })?;
    ["ggml-base.en.bin", "base.en.bin", "ggml-base.en.gguf"]
        .into_iter()
        .map(|name| dir.join(name))
        .find(|path| path.is_file())
}

fn on_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
        .unwrap_or(false)
}

fn is_wayland() -> bool {
    matches!(
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
        Some("wayland")
    ) || std::env::var_os("WAYLAND_DISPLAY").is_some()
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn main() {
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
