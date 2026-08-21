use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::inject::InjectReport;
use crate::paths::history_path;
use crate::types::EngineId;

const HISTORY_CAP: usize = 2000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryRow {
    pub id: String,
    pub text: String,
    pub raw: String,
    pub engine: EngineId,
    pub started_at: u64,
    pub infer_ms: u64,
    pub inject: InjectReport,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct HistoryFile {
    rows: Vec<HistoryRow>,
}

#[derive(Debug, Clone)]
pub struct History {
    rows: Vec<HistoryRow>,
    path: PathBuf,
}

impl History {
    pub fn load() -> Result<Self, String> {
        Self::load_from(history_path())
    }

    pub fn load_from(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref().to_path_buf();
        if !path.exists() {
            return Ok(Self {
                rows: Vec::new(),
                path,
            });
        }
        let raw = fs::read_to_string(&path).map_err(|err| err.to_string())?;
        let file: HistoryFile = serde_json::from_str(&raw).map_err(|err| err.to_string())?;
        Ok(Self {
            rows: file.rows,
            path,
        })
    }

    #[must_use]
    pub fn rows(&self) -> &[HistoryRow] {
        &self.rows
    }

    pub fn append(&mut self, row: HistoryRow) -> Result<(), String> {
        self.rows.push(row);
        if self.rows.len() > HISTORY_CAP {
            let drop = self.rows.len() - HISTORY_CAP;
            self.rows.drain(..drop);
        }
        self.save()
    }

    pub fn save(&self) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        let file = HistoryFile {
            rows: self.rows.clone(),
        };
        let raw = serde_json::to_string_pretty(&file).map_err(|err| err.to_string())?;
        fs::write(&self.path, raw).map_err(|err| err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inject::InjectBackend;

    #[test]
    fn persists_across_reload() {
        let dir = std::env::temp_dir().join(format!("echo-hist-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("history.json");
        let mut store = History::load_from(&path).unwrap();
        store
            .append(HistoryRow {
                id: "1".into(),
                text: "hello".into(),
                raw: "hello".into(),
                engine: EngineId::Whisper {
                    model: "fake".into(),
                },
                started_at: 1,
                infer_ms: 2,
                inject: InjectReport::Typed {
                    backend: InjectBackend::Xdotool,
                },
            })
            .unwrap();
        let reloaded = History::load_from(&path).unwrap();
        assert_eq!(reloaded.rows().len(), 1);
        assert_eq!(reloaded.rows()[0].text, "hello");
    }
}
