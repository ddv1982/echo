use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::paths::{dictionary_path, set_aside_corrupt, write_atomic};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RecognitionHints {
    terms: Vec<String>,
}

impl RecognitionHints {
    pub const MAX_TERMS: usize = 32;
    pub const MAX_PROMPT_BYTES: usize = 512;

    #[must_use]
    pub fn from_dictionary(dictionary: &Dictionary) -> Self {
        let mut entries: Vec<(usize, &DictEntry)> = dictionary.entries.iter().enumerate().collect();
        entries.sort_by(|(left_index, left), (right_index, right)| {
            right
                .created_at
                .cmp(&left.created_at)
                .then_with(|| right_index.cmp(left_index))
        });

        let mut terms = Vec::new();
        let mut keys = std::collections::HashSet::new();
        let mut prompt_bytes = 0;
        for (_, entry) in entries {
            if entry.written.chars().any(char::is_control) {
                continue;
            }
            let term = entry
                .written
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            if term.is_empty() || term.len() > Self::MAX_PROMPT_BYTES {
                continue;
            }
            let key = term.to_lowercase();
            if !keys.insert(key) {
                continue;
            }
            let separator_bytes = usize::from(!terms.is_empty()) * 2;
            if prompt_bytes + separator_bytes + term.len() > Self::MAX_PROMPT_BYTES {
                continue;
            }
            prompt_bytes += separator_bytes + term.len();
            terms.push(term);
            if terms.len() == Self::MAX_TERMS {
                break;
            }
        }
        Self { terms }
    }

    #[must_use]
    pub fn terms(&self) -> &[String] {
        &self.terms
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DictEntry {
    pub spoken: String,
    pub written: String,
    pub created_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictionaryConflict {
    pub spoken: String,
    pub written: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictionaryBatchOutcome {
    pub added: usize,
    pub unchanged: usize,
    pub conflicts: Vec<DictionaryConflict>,
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

    pub fn load_read_only() -> Result<Self, String> {
        Self::load_from_read_only(dictionary_path())
    }

    pub fn load_from_read_only(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref().to_path_buf();
        if !path.exists() {
            return Ok(Self {
                entries: Vec::new(),
                path,
            });
        }
        let raw = fs::read_to_string(&path).map_err(|err| err.to_string())?;
        let entries = serde_json::from_str::<DictFile>(&raw)
            .map(|file| file.entries)
            .unwrap_or_default();
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

    pub fn add_batch(
        &mut self,
        written: &str,
        spoken_variants: impl IntoIterator<Item = String>,
    ) -> Result<DictionaryBatchOutcome, String> {
        let written = clean_phrase(written);
        if written.is_empty() {
            return Err("The written form is required.".to_string());
        }

        let canonical_key = phrase_key(&written);
        let mut seen = std::collections::HashSet::new();
        let mut candidates = Vec::new();
        let mut unchanged = 0;

        for spoken in spoken_variants {
            let spoken = clean_phrase(&spoken);
            if spoken.is_empty() {
                unchanged += 1;
                continue;
            }
            let key = phrase_key(&spoken);
            if key == canonical_key || !seen.insert(key.clone()) {
                unchanged += 1;
                continue;
            }
            candidates.push((key, spoken));
        }

        let mut additions = Vec::new();
        let mut conflicts = Vec::new();
        for (key, spoken) in candidates {
            let matching = self
                .entries
                .iter()
                .filter(|entry| phrase_key(&entry.spoken) == key)
                .collect::<Vec<_>>();
            if matching.is_empty() {
                additions.push(spoken);
                continue;
            }
            if matching
                .iter()
                .all(|entry| clean_phrase(&entry.written) == written)
            {
                unchanged += 1;
                continue;
            }
            for entry in matching {
                if clean_phrase(&entry.written) != written {
                    conflicts.push(DictionaryConflict {
                        spoken: spoken.clone(),
                        written: entry.written.clone(),
                    });
                }
            }
        }

        conflicts.sort_by(|left, right| {
            phrase_key(&left.spoken)
                .cmp(&phrase_key(&right.spoken))
                .then_with(|| left.written.cmp(&right.written))
        });
        conflicts.dedup();
        if !conflicts.is_empty() {
            return Ok(DictionaryBatchOutcome {
                added: 0,
                unchanged,
                conflicts,
            });
        }

        let added = additions.len();
        let created_at = now_secs();
        let original_len = self.entries.len();
        self.entries
            .extend(additions.into_iter().map(|spoken| DictEntry {
                spoken,
                written: written.clone(),
                created_at,
            }));
        if added > 0 {
            if let Err(error) = self.save() {
                self.entries.truncate(original_len);
                return Err(error);
            }
        }
        Ok(DictionaryBatchOutcome {
            added,
            unchanged,
            conflicts: Vec::new(),
        })
    }

    pub fn save(&self) -> Result<(), String> {
        let file = DictFile {
            entries: self.entries.clone(),
        };
        let raw = serde_json::to_string_pretty(&file).map_err(|err| err.to_string())?;
        write_atomic(&self.path, raw.as_bytes())
    }

    #[must_use]
    pub fn rewrite(&self, text: &str) -> String {
        if text.is_empty() {
            return String::new();
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
        apply_hits(text, &hits)
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

fn clean_phrase(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn phrase_key(value: &str) -> String {
    clean_phrase(value).to_lowercase()
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

    fn persisted_dict(entries: &[(&str, &str)]) -> Dictionary {
        let path = std::env::temp_dir().join(format!(
            "echo-dictionary-batch-{}-{}.json",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let mut dictionary = dict(entries);
        dictionary.path = path;
        dictionary.save().unwrap();
        dictionary
    }

    #[test]
    fn hit_rewrites_phrase() {
        let d = dict(&[("clawed code", "Claude Code")]);
        let rewrite = d.rewrite("open clawed code please");
        assert_eq!(rewrite, "open Claude Code please");
    }

    #[test]
    fn miss_leaves_text() {
        let d = dict(&[("clawed code", "Claude Code")]);
        let rewrite = d.rewrite("open the editor");
        assert_eq!(rewrite, "open the editor");
    }

    #[test]
    fn longest_whole_phrase_wins() {
        let d = dict(&[("code", "CODE"), ("clawed code", "Claude Code")]);
        let rewrite = d.rewrite("clawed code and more code");
        assert_eq!(rewrite, "Claude Code and more CODE");
    }

    #[test]
    fn does_not_match_inside_words() {
        let d = dict(&[("code", "CODE")]);
        let rewrite = d.rewrite("codebase uses code");
        assert_eq!(rewrite, "codebase uses CODE");
    }

    #[test]
    fn batch_deduplicates_and_is_idempotent() {
        let mut dictionary = persisted_dict(&[("already heard", "Canonical")]);
        let outcome = dictionary
            .add_batch(
                "  Canonical ",
                [
                    "new hearing".to_string(),
                    " NEW   HEARING ".to_string(),
                    "Canonical".to_string(),
                    "already heard".to_string(),
                ],
            )
            .unwrap();

        assert_eq!(outcome.added, 1);
        assert_eq!(outcome.unchanged, 3);
        assert!(outcome.conflicts.is_empty());
        assert_eq!(dictionary.entries().len(), 2);

        let second = dictionary
            .add_batch("Canonical", ["new hearing".to_string()])
            .unwrap();
        assert_eq!(second.added, 0);
        assert_eq!(second.unchanged, 1);
        assert_eq!(
            Dictionary::load_from(&dictionary.path)
                .unwrap()
                .entries()
                .len(),
            2
        );
        let _ = fs::remove_file(&dictionary.path);
    }

    #[test]
    fn batch_conflict_rolls_back_every_candidate() {
        let mut dictionary = persisted_dict(&[("shared sound", "Existing")]);
        let before = fs::read_to_string(&dictionary.path).unwrap();
        let outcome = dictionary
            .add_batch(
                "Canonical",
                ["new hearing".to_string(), "shared sound".to_string()],
            )
            .unwrap();

        assert_eq!(outcome.added, 0);
        assert_eq!(
            outcome.conflicts,
            vec![DictionaryConflict {
                spoken: "shared sound".to_string(),
                written: "Existing".to_string(),
            }]
        );
        assert_eq!(dictionary.entries().len(), 1);
        assert_eq!(fs::read_to_string(&dictionary.path).unwrap(), before);
        let _ = fs::remove_file(&dictionary.path);
    }

    #[test]
    fn batch_write_failure_keeps_in_memory_entries_unchanged() {
        let parent = std::env::temp_dir().join(format!(
            "echo-dictionary-batch-blocked-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::write(&parent, "not a directory").unwrap();
        let mut dictionary = dict(&[("existing", "Existing")]);
        dictionary.path = parent.join("dictionary.json");

        let result = dictionary.add_batch("Canonical", ["new hearing".to_string()]);

        assert!(result.is_err());
        assert_eq!(
            dictionary.entries(),
            dict(&[("existing", "Existing")]).entries()
        );
        let _ = fs::remove_file(parent);
    }

    #[test]
    fn empty_input() {
        let d = dict(&[("code", "CODE")]);
        let rewrite = d.rewrite("");
        assert_eq!(rewrite, "");
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
    fn recognition_hints_are_newest_written_unique_and_normalized() {
        let d = dict(&[
            ("clawed code", "Claude   Code"),
            ("old", "Echo"),
            ("new", "echo"),
            ("spoken-only", "New term"),
        ]);
        let hints = RecognitionHints::from_dictionary(&d);
        assert_eq!(hints.terms(), ["New term", "echo", "Claude Code"]);
        assert!(!hints.terms().iter().any(|term| term == "spoken-only"));
    }

    #[test]
    fn recognition_hints_reject_controls_and_bound_whole_utf8_terms() {
        let mut entries = vec![DictEntry {
            spoken: "bad".into(),
            written: "line\nbreak".into(),
            created_at: 100,
        }];
        entries.push(DictEntry {
            spoken: "too long".into(),
            written: "x".repeat(RecognitionHints::MAX_PROMPT_BYTES + 1),
            created_at: 99,
        });
        for index in 0..40 {
            entries.push(DictEntry {
                spoken: format!("spoken {index}"),
                written: format!("term {index} é"),
                created_at: index,
            });
        }
        let d = Dictionary {
            path: PathBuf::from("/tmp/echo-dict-unused.json"),
            entries,
        };
        let hints = RecognitionHints::from_dictionary(&d);
        assert_eq!(hints.terms().len(), RecognitionHints::MAX_TERMS);
        let prompt = hints.terms().join(", ");
        assert!(prompt.len() <= RecognitionHints::MAX_PROMPT_BYTES);
        assert!(!prompt.contains("line"));
        assert!(prompt.is_char_boundary(prompt.len()));
        assert!(hints.terms().iter().all(|term| term.ends_with('é')));
    }

    #[test]
    fn read_only_corrupt_dictionary_does_not_move_or_rewrite_it() {
        let path = std::env::temp_dir().join(format!(
            "echo-dict-read-only-corrupt-{}.json",
            std::process::id()
        ));
        fs::write(&path, "not json").unwrap();
        let loaded = Dictionary::load_from_read_only(&path).unwrap();
        assert!(loaded.entries().is_empty());
        assert_eq!(fs::read_to_string(&path).unwrap(), "not json");
        assert!(!path.with_extension("json.corrupt").exists());
        let _ = fs::remove_file(path);
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
