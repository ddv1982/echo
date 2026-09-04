use echo::inject::{Pasteboard, SysClipboard};
use echo_core::{Dictionary, FailReason, History, InjectBackend, InjectReport};
use echo_desktop::ipc::{DictionaryBatchResult, DictionaryItem, HistoryItem};

fn injection_backend_name(backend: InjectBackend) -> &'static str {
    match backend {
        InjectBackend::Ydotool => "Ydotool",
        InjectBackend::Xdotool => "Xdotool",
        InjectBackend::Wtype => "Wtype",
    }
}

fn injection_failure_name(reason: FailReason) -> &'static str {
    match reason {
        FailReason::NoInputDevice => "NoInputDevice",
        FailReason::CaptureFailed => "CaptureFailed",
        FailReason::InjectPermission => "InjectPermission",
        FailReason::EngineMissing => "EngineMissing",
        FailReason::NoFocus => "NoFocus",
        FailReason::EngineError => "EngineError",
        FailReason::InjectUnconfirmed => "InjectUnconfirmed",
    }
}

fn injection_outcome(report: &InjectReport) -> String {
    match report {
        InjectReport::Typed { backend } => {
            format!("Typed {{ backend: {} }}", injection_backend_name(*backend))
        }
        InjectReport::Pasted { backend } => {
            format!("Pasted {{ backend: {} }}", injection_backend_name(*backend))
        }
        InjectReport::ClipboardOnly => "ClipboardOnly".to_string(),
        InjectReport::Failed { reason } => {
            format!("Failed {{ reason: {} }}", injection_failure_name(*reason))
        }
    }
}

#[tauri::command]
pub(crate) async fn get_history() -> Result<Vec<HistoryItem>, String> {
    crate::blocking::run_blocking("history load", || {
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
                injection: injection_outcome(&row.inject),
            })
            .collect())
    })
    .await?
}

#[tauri::command]
pub(crate) async fn delete_history_item(id: String) -> Result<bool, String> {
    crate::blocking::run_blocking("history deletion", move || {
        let removed = History::remove_default(&id)?;
        crate::status::last_run_invalidate();
        Ok(removed)
    })
    .await?
}

#[tauri::command]
pub(crate) async fn clear_history() -> Result<usize, String> {
    crate::blocking::run_blocking("history clear", || {
        let cleared = History::clear_default()?;
        crate::status::last_run_invalidate();
        Ok(cleared)
    })
    .await?
}

#[tauri::command]
pub(crate) async fn get_dictionary() -> Result<Vec<DictionaryItem>, String> {
    crate::blocking::run_blocking("dictionary load", || {
        let dictionary = Dictionary::load()?;
        Ok(dictionary
            .entries()
            .iter()
            .map(DictionaryItem::from)
            .collect())
    })
    .await?
}

#[tauri::command]
pub(crate) async fn add_dictionary_entry(
    spoken: String,
    written: String,
) -> Result<DictionaryItem, String> {
    crate::blocking::run_blocking("dictionary entry addition", move || {
        let spoken = spoken.trim().to_string();
        let written = written.trim().to_string();
        if spoken.is_empty() || written.is_empty() {
            return Err("Both spoken and written forms are required.".to_string());
        }
        let mut dictionary = Dictionary::load()?;
        dictionary
            .add(spoken, written)
            .map(|entry| DictionaryItem::from(&entry))
    })
    .await?
}

#[tauri::command]
pub(crate) async fn add_dictionary_entries_batch(
    written: String,
    spoken: Vec<String>,
) -> Result<DictionaryBatchResult, String> {
    crate::blocking::run_blocking("dictionary batch addition", move || {
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
    })
    .await?
}

#[tauri::command]
pub(crate) async fn remove_dictionary_entry(
    spoken: String,
    written: String,
) -> Result<bool, String> {
    crate::blocking::run_blocking("dictionary entry removal", move || {
        Dictionary::load()?.remove(&spoken, &written)
    })
    .await?
}

#[tauri::command]
pub(crate) fn copy_text(text: String) -> Result<(), String> {
    SysClipboard.set(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn injection_outcomes_preserve_the_existing_explicit_labels() {
        for (backend, name) in [
            (InjectBackend::Ydotool, "Ydotool"),
            (InjectBackend::Xdotool, "Xdotool"),
            (InjectBackend::Wtype, "Wtype"),
        ] {
            assert_eq!(
                injection_outcome(&InjectReport::Typed { backend }),
                format!("Typed {{ backend: {name} }}")
            );
            assert_eq!(
                injection_outcome(&InjectReport::Pasted { backend }),
                format!("Pasted {{ backend: {name} }}")
            );
        }

        assert_eq!(
            injection_outcome(&InjectReport::ClipboardOnly),
            "ClipboardOnly"
        );

        for (reason, name) in [
            (FailReason::NoInputDevice, "NoInputDevice"),
            (FailReason::CaptureFailed, "CaptureFailed"),
            (FailReason::InjectPermission, "InjectPermission"),
            (FailReason::EngineMissing, "EngineMissing"),
            (FailReason::NoFocus, "NoFocus"),
            (FailReason::EngineError, "EngineError"),
            (FailReason::InjectUnconfirmed, "InjectUnconfirmed"),
        ] {
            assert_eq!(
                injection_outcome(&InjectReport::Failed { reason }),
                format!("Failed {{ reason: {name} }}")
            );
        }
    }
}
