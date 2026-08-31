use echo::inject::{Pasteboard, SysClipboard};
use echo_core::{Dictionary, History};
use echo_desktop::ipc::{DictionaryBatchResult, DictionaryItem, HistoryItem};
use std::sync::Mutex;

static DICTIONARY_WRITES: Mutex<()> = Mutex::new(());

fn dictionary_write_guard() -> Result<std::sync::MutexGuard<'static, ()>, String> {
    DICTIONARY_WRITES
        .lock()
        .map_err(|_| "Dictionary writes are unavailable.".to_string())
}

#[tauri::command]
pub(crate) fn get_history() -> Result<Vec<HistoryItem>, String> {
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
pub(crate) fn get_dictionary() -> Result<Vec<DictionaryItem>, String> {
    let dictionary = Dictionary::load()?;
    Ok(dictionary
        .entries()
        .iter()
        .map(DictionaryItem::from)
        .collect())
}

#[tauri::command]
pub(crate) fn add_dictionary_entry(
    spoken: String,
    written: String,
) -> Result<DictionaryItem, String> {
    let spoken = spoken.trim().to_string();
    let written = written.trim().to_string();
    if spoken.is_empty() || written.is_empty() {
        return Err("Both spoken and written forms are required.".to_string());
    }
    let _guard = dictionary_write_guard()?;
    let mut dictionary = Dictionary::load()?;
    dictionary
        .add(spoken, written)
        .map(|entry| DictionaryItem::from(&entry))
}

#[tauri::command]
pub(crate) fn add_dictionary_entries_batch(
    written: String,
    spoken: Vec<String>,
) -> Result<DictionaryBatchResult, String> {
    let _guard = dictionary_write_guard()?;
    let mut dictionary = Dictionary::load()?;
    let outcome = dictionary.add_batch(&written, spoken)?;
    Ok(DictionaryBatchResult {
        entries: dictionary
            .entries()
            .iter()
            .map(DictionaryItem::from)
            .collect(),
        added: outcome.added,
        unchanged: outcome.unchanged,
        conflicts: outcome.conflicts.into_iter().map(Into::into).collect(),
    })
}

#[tauri::command]
pub(crate) fn remove_dictionary_entry(spoken: String, written: String) -> Result<bool, String> {
    let _guard = dictionary_write_guard()?;
    Dictionary::load()?.remove(&spoken, &written)
}

#[tauri::command]
pub(crate) fn copy_text(text: String) -> Result<(), String> {
    SysClipboard.set(&text)
}
