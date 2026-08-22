use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::paths::{dictionary_path, set_aside_corrupt, write_atomic};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DictEntry {
    pub spoken: String,
    pub written: String,
    pub created_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rewrite {
    pub text: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct DictFile {
    entries: Vec<DictEntry>,
}

#[derive(Debug, Clone)]
pub struct Dictionary {
    entries: Vec<DictEntry>,
    path: PathBuf,
}

impl Dictionary {
    pub fn load() -> Result<Self, String> {
        Self::load_from(dictionary_path())
    }

    #[must_use]
    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
            path: dictionary_path(),
        }
    }

    pub fn load_from(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref().to_path_buf();
        if !path.exists() {
            return Ok(Self {
                entries: Vec::new(),
                path,
            });
        }
        let raw = fs::read_to_string(&path).map_err(|err| err.to_string())?;
        let entries = match serde_json::from_str::<DictFile>(&raw) {
            Ok(file) => file.entries,
            Err(_) => {
                set_aside_corrupt(&path);
                Vec::new()
            }
        };
        Ok(Self { entries, path })
    }

    #[must_use]
    pub fn entries(&self) -> &[DictEntry] {
        &self.entries
    }

    pub fn add(
        &mut self,
        spoken: impl Into<String>,
        written: impl Into<String>,
    ) -> Result<DictEntry, String> {
        let entry = DictEntry {
            spoken: spoken.into(),
            written: written.into(),
            created_at: now_secs(),
        };
        self.entries.push(entry.clone());
        self.save()?;
        Ok(entry)
    }

    pub fn remove(&mut self, spoken: &str, written: &str) -> Result<bool, String> {
        let original_len = self.entries.len();
        self.entries
            .retain(|entry| entry.spoken != spoken || entry.written != written);
        let removed = self.entries.len() != original_len;
        if removed {
            self.save()?;
        }
        Ok(removed)
    }

    pub fn save(&self) -> Result<(), String> {
        let file = DictFile {
            entries: self.entries.clone(),
        };
        let raw = serde_json::to_string_pretty(&file).map_err(|err| err.to_string())?;
        write_atomic(&self.path, raw.as_bytes())
    }

    #[must_use]
    pub fn rewrite(&self, text: &str) -> Rewrite {
        if text.is_empty() {
            return Rewrite {
                text: String::new(),
            };
        }
        let hay = text.to_ascii_lowercase();
        let mut taken = vec![false; text.len()];
        let mut hits: Vec<(usize, usize, &str)> = Vec::new();
        let mut ranked: Vec<&DictEntry> = self.entries.iter().collect();
        ranked.sort_by(|a, b| {
            b.spoken
                .chars()
                .count()
                .cmp(&a.spoken.chars().count())
                .then_with(|| a.created_at.cmp(&b.created_at))
        });
        for entry in ranked {
            if entry.spoken.is_empty() {
                continue;
            }
            let needle = entry.spoken.to_ascii_lowercase();
            let mut from = 0;
            while let Some(rel) = hay.get(from..).and_then(|rest| rest.find(&needle)) {
                let start = from + rel;
                let end = start + needle.len();
                if end > text.len() || taken[start..end].iter().any(|flag| *flag) {
                    from = start + 1;
                    continue;
                }
                if !whole_phrase(text, start, end) {
                    from = start + 1;
                    continue;
                }
                for flag in &mut taken[start..end] {
                    *flag = true;
                }
                hits.push((start, end, entry.written.as_str()));
                from = end;
            }
        }
        hits.sort_by_key(|hit| hit.0);
        Rewrite {
            text: apply_hits(text, &hits),
        }
    }
}

fn apply_hits(text: &str, hits: &[(usize, usize, &str)]) -> String {
    let mut out = String::new();
    let mut cursor = 0;
    for (start, end, written) in hits {
        if *start >= cursor {
            out.push_str(&text[cursor..*start]);
            out.push_str(written);
            cursor = *end;
        }
    }
    out.push_str(&text[cursor..]);
    out
}

fn whole_phrase(text: &str, start: usize, end: usize) -> bool {
    let left = text.get(..start).and_then(|s| s.chars().next_back());
    let right = text.get(end..).and_then(|s| s.chars().next());
    left.map(|c| !is_word_char(c)).unwrap_or(true)
        && right.map(|c| !is_word_char(c)).unwrap_or(true)
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '\''
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dict(entries: &[(&str, &str)]) -> Dictionary {
        Dictionary {
            path: PathBuf::from("/tmp/echo-dict-unused.json"),
            entries: entries
                .iter()
                .enumerate()
                .map(|(i, (spoken, written))| DictEntry {
                    spoken: (*spoken).to_string(),
                    written: (*written).to_string(),
                    created_at: i as u64,
                })
                .collect(),
        }
    }

    #[test]
    fn hit_rewrites_phrase() {
        let d = dict(&[("clawed code", "Claude Code")]);
        let rewrite = d.rewrite("open clawed code please");
        assert_eq!(rewrite.text, "open Claude Code please");
    }

    #[test]
    fn miss_leaves_text() {
        let d = dict(&[("clawed code", "Claude Code")]);
        let rewrite = d.rewrite("open the editor");
        assert_eq!(rewrite.text, "open the editor");
    }

    #[test]
    fn longest_whole_phrase_wins() {
        let d = dict(&[("code", "CODE"), ("clawed code", "Claude Code")]);
        let rewrite = d.rewrite("clawed code and more code");
        assert_eq!(rewrite.text, "Claude Code and more CODE");
    }

    #[test]
    fn does_not_match_inside_words() {
        let d = dict(&[("code", "CODE")]);
        let rewrite = d.rewrite("codebase uses code");
        assert_eq!(rewrite.text, "codebase uses CODE");
    }

    #[test]
    fn empty_input() {
        let d = dict(&[("code", "CODE")]);
        let rewrite = d.rewrite("");
        assert_eq!(rewrite.text, "");
    }

    #[test]
    fn persists_under_data_dir() {
        let dir = std::env::temp_dir().join(format!("echo-dict-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dictionary.json");
        let mut store = Dictionary::load_from(&path).unwrap();
        store.add("clawed code", "Claude Code").unwrap();
        let reloaded = Dictionary::load_from(&path).unwrap();
        assert_eq!(reloaded.entries().len(), 1);
        assert_eq!(reloaded.entries()[0].written, "Claude Code");
    }

    #[test]
    fn removes_exact_entry_and_persists() {
        let dir = std::env::temp_dir().join(format!("echo-dict-remove-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dictionary.json");
        let mut store = Dictionary::load_from(&path).unwrap();
        store.add("clawed code", "Claude Code").unwrap();
        store.add("echo", "Echo").unwrap();

        assert!(store.remove("clawed code", "Claude Code").unwrap());
        assert!(!store.remove("missing", "Missing").unwrap());
        let reloaded = Dictionary::load_from(&path).unwrap();
        assert_eq!(reloaded.entries().len(), 1);
        assert_eq!(reloaded.entries()[0].written, "Echo");
    }

    #[test]
    fn corrupt_file_is_set_aside_not_fatal() {
        let dir = std::env::temp_dir().join(format!("echo-dict-corrupt-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dictionary.json");
        fs::write(&path, "not json at all").unwrap();

        let mut store = Dictionary::load_from(&path).unwrap();
        assert!(store.entries().is_empty());
        assert!(!path.exists(), "corrupt file should be moved aside");
        assert!(dir.join("dictionary.json.corrupt").exists());

        store.add("clawed code", "Claude Code").unwrap();
        assert_eq!(Dictionary::load_from(&path).unwrap().entries().len(), 1);
    }
}
