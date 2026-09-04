use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use caseless::Caseless;
use fs2::FileExt;
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

use crate::paths::{
    dictionary_path, read_private, set_aside_corrupt, write_atomic_private, PrivateDir,
};

const MATCH_CHUNK_ENTRIES: usize = 64;
const MATCH_CHUNK_PATTERN_BYTES: usize = 16 * 1024;
static DICTIONARY_WRITES: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy)]
struct MatchLimits {
    entries: usize,
    pattern_bytes: usize,
    regex_size_limit: Option<usize>,
}

enum PhraseMatcher {
    Regex {
        regex: Regex,
        entry_indices: Vec<usize>,
    },
    SingleRegex {
        regex: Regex,
        entry_index: usize,
    },
    Literal {
        entry_index: usize,
        needle: String,
    },
}

struct FoldedText {
    text: String,
    original_boundaries: Vec<Option<usize>>,
}

impl FoldedText {
    fn new(original: &str) -> Self {
        let mut text = String::new();
        let mut original_boundaries = vec![Some(0)];
        for (start, grapheme) in original.grapheme_indices(true) {
            let folded = canonical_fold(grapheme);
            let folded_start = text.len();
            text.push_str(&folded);
            original_boundaries.resize(text.len() + 1, None);
            original_boundaries[folded_start] = Some(start);
            original_boundaries[text.len()] = Some(start + grapheme.len());
        }
        Self {
            text,
            original_boundaries,
        }
    }

    fn original_range(&self, start: usize, end: usize) -> Option<(usize, usize)> {
        Some((
            self.original_boundaries.get(start).copied().flatten()?,
            self.original_boundaries.get(end).copied().flatten()?,
        ))
    }
}

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
            let key = canonical_fold(&term);
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

#[derive(Debug, Clone, Default, Serialize)]
struct DictFile {
    entries: Vec<DictEntry>,
}

impl<'de> Deserialize<'de> for DictFile {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct CurrentDictFile {
            entries: Vec<DictEntry>,
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum DictFileWire {
            Current(CurrentDictFile),
            Other(std::collections::BTreeMap<String, serde::de::IgnoredAny>),
        }

        match DictFileWire::deserialize(deserializer)? {
            DictFileWire::Current(file) => Ok(Self {
                entries: file.entries,
            }),
            DictFileWire::Other(fields) if fields.is_empty() => Ok(Self::default()),
            DictFileWire::Other(_) => Err(serde::de::Error::custom(
                "dictionary object is missing its entries field",
            )),
        }
    }
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
        let Some(raw) = read_private(&path)? else {
            return Ok(Self {
                entries: Vec::new(),
                path,
            });
        };
        let entries = match serde_json::from_slice::<DictFile>(&raw) {
            Ok(file) => file.entries,
            Err(error) => {
                set_aside_corrupt(&path)?;
                return Err(format!(
                    "Dictionary file {} contains invalid JSON and was not loaded: {error}",
                    path.display()
                ));
            }
        };
        Ok(Self { entries, path })
    }

    pub fn load_read_only() -> Result<Self, String> {
        Self::load_from_read_only(dictionary_path())
    }

    pub fn load_from_read_only(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref().to_path_buf();
        let Some(raw) = read_private(&path)? else {
            return Ok(Self {
                entries: Vec::new(),
                path,
            });
        };
        let entries = serde_json::from_slice::<DictFile>(&raw)
            .map(|file| file.entries)
            .map_err(|error| {
                format!(
                    "Dictionary file {} contains invalid JSON and was not loaded: {error}",
                    path.display()
                )
            })?;
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
        self.update_locked(move |entries| {
            entries.push(entry.clone());
            Ok((entry, true))
        })
    }

    pub fn remove(&mut self, spoken: &str, written: &str) -> Result<bool, String> {
        self.update_locked(|entries| {
            let original_len = entries.len();
            entries.retain(|entry| entry.spoken != spoken || entry.written != written);
            let removed = entries.len() != original_len;
            Ok((removed, removed))
        })
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
        let spoken_variants = spoken_variants.into_iter();

        self.update_locked(move |entries| {
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
                let matching = entries
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
                return Ok((
                    DictionaryBatchOutcome {
                        added: 0,
                        unchanged,
                        conflicts,
                    },
                    false,
                ));
            }

            let added = additions.len();
            let created_at = now_secs();
            entries.extend(additions.into_iter().map(|spoken| DictEntry {
                spoken,
                written: written.clone(),
                created_at,
            }));
            Ok((
                DictionaryBatchOutcome {
                    added,
                    unchanged,
                    conflicts: Vec::new(),
                },
                added > 0,
            ))
        })
    }

    pub fn save(&self) -> Result<(), String> {
        let _process_guard = dictionary_process_guard()?;
        let _file_guard = lock_dictionary_file(&self.path)?;
        let entries = match read_private(&self.path)? {
            Some(raw) => serde_json::from_slice::<DictFile>(&raw)
                .map(|file| file.entries)
                .map_err(|error| {
                    format!(
                        "Dictionary file {} contains invalid JSON and was not loaded: {error}",
                        self.path.display()
                    )
                })?,
            None => self.entries.clone(),
        };
        self.save_entries(&entries)
    }

    fn save_entries(&self, entries: &[DictEntry]) -> Result<(), String> {
        let file = DictFile {
            entries: entries.to_vec(),
        };
        let raw = serde_json::to_string_pretty(&file).map_err(|err| err.to_string())?;
        write_atomic_private(&self.path, raw.as_bytes())
    }

    fn update_locked<T>(
        &mut self,
        update: impl FnOnce(&mut Vec<DictEntry>) -> Result<(T, bool), String>,
    ) -> Result<T, String> {
        let _process_guard = dictionary_process_guard()?;
        let _file_guard = lock_dictionary_file(&self.path)?;
        let mut entries = Self::load_from(&self.path)?.entries;
        let (result, changed) = update(&mut entries)?;
        if changed {
            self.save_entries(&entries)?;
        }
        self.entries = entries;
        Ok(result)
    }

    #[must_use]
    pub fn rewrite(&self, text: &str) -> String {
        self.rewrite_with_limits(
            text,
            MatchLimits {
                entries: MATCH_CHUNK_ENTRIES,
                pattern_bytes: MATCH_CHUNK_PATTERN_BYTES,
                regex_size_limit: None,
            },
        )
    }

    fn rewrite_with_limits(&self, text: &str, limits: MatchLimits) -> String {
        if text.is_empty() {
            return String::new();
        }
        let mut taken = vec![false; text.len()];
        let mut hits: Vec<(usize, usize, &str)> = Vec::new();
        let folded_text = FoldedText::new(text);
        let mut ranked: Vec<(usize, &DictEntry)> = self.entries.iter().enumerate().collect();
        ranked.sort_by(|(left_index, left), (right_index, right)| {
            right
                .spoken
                .chars()
                .count()
                .cmp(&left.spoken.chars().count())
                .then_with(|| left_index.cmp(right_index))
        });

        let ranked_indices = ranked
            .into_iter()
            .filter_map(|(index, entry)| (!entry.spoken.is_empty()).then_some(index))
            .collect::<Vec<_>>();
        let matchers = compile_phrase_matchers(&self.entries, &ranked_indices, limits);

        for matcher in matchers {
            let mut candidates = matcher_candidates(&matcher, &folded_text.text);
            candidates.sort_by_key(|(priority, start, _)| (*priority, *start));
            for (priority, folded_start, folded_end) in candidates {
                let Some((start, end)) = folded_text.original_range(folded_start, folded_end)
                else {
                    continue;
                };
                if taken[start..end].iter().any(|flag| *flag) {
                    continue;
                }
                if !whole_phrase(text, start, end) {
                    continue;
                }
                for flag in &mut taken[start..end] {
                    *flag = true;
                }
                let entry_index = matcher_entry_index(&matcher, priority);
                hits.push((start, end, self.entries[entry_index].written.as_str()));
            }
        }
        hits.sort_by_key(|hit| hit.0);
        apply_hits(text, &hits)
    }
}

fn dictionary_process_guard() -> Result<std::sync::MutexGuard<'static, ()>, String> {
    DICTIONARY_WRITES.lock().map_err(|_| {
        "Dictionary writes are unavailable because the dictionary lock is poisoned.".to_string()
    })
}

fn lock_dictionary_file(path: &Path) -> Result<fs::File, String> {
    let parent = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let directory = PrivateDir::open(parent).map_err(|err| {
        format!(
            "Could not create dictionary lock directory {}: {err}",
            parent.display()
        )
    })?;
    let mut lock_name = path
        .file_name()
        .ok_or_else(|| format!("Could not derive a lock file for {}", path.display()))?
        .to_os_string();
    lock_name.push(".lock");
    let lock_path = parent.join(lock_name);
    let lock_file = directory
        .open_or_create(lock_path.file_name().expect("lock path has a file name"))
        .map_err(|err| {
            format!(
                "Could not open dictionary lock {}: {err}",
                lock_path.display()
            )
        })?;
    match lock_file.try_lock_exclusive() {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
            #[cfg(test)]
            report_dictionary_lock_contention();
            lock_file.lock_exclusive().map_err(|err| {
                format!(
                    "Could not lock dictionary file {}: {err}",
                    lock_path.display()
                )
            })?;
        }
        Err(err) => {
            return Err(format!(
                "Could not lock dictionary file {}: {err}",
                lock_path.display()
            ));
        }
    }
    Ok(lock_file)
}

#[cfg(test)]
fn report_dictionary_lock_contention() {
    let Some(address) = std::env::var_os("ECHO_TEST_DICTIONARY_CONTENTION_ADDRESS") else {
        return;
    };
    let mut stream = std::net::TcpStream::connect(address.to_string_lossy().as_ref())
        .expect("dictionary contention observer should accept a connection");
    std::io::Write::write_all(&mut stream, b"dictionary lock contended")
        .expect("dictionary contention marker should be written");
}

fn compile_phrase_matchers(
    entries: &[DictEntry],
    ranked_indices: &[usize],
    limits: MatchLimits,
) -> Vec<PhraseMatcher> {
    let max_entries = limits.entries.max(1);
    let max_pattern_bytes = limits.pattern_bytes.max(1);
    let mut matchers = Vec::new();
    let mut chunk_start = 0;
    while chunk_start < ranked_indices.len() {
        let mut chunk_end = chunk_start;
        let mut pattern_bytes = 0usize;
        while chunk_end < ranked_indices.len() && chunk_end - chunk_start < max_entries {
            let escaped_bytes =
                regex::escape(&canonical_fold(&entries[ranked_indices[chunk_end]].spoken)).len();
            if chunk_end > chunk_start
                && pattern_bytes.saturating_add(escaped_bytes) > max_pattern_bytes
            {
                break;
            }
            pattern_bytes = pattern_bytes.saturating_add(escaped_bytes);
            chunk_end += 1;
        }
        compile_matcher_chunk(
            entries,
            &ranked_indices[chunk_start..chunk_end],
            limits.regex_size_limit,
            &mut matchers,
        );
        chunk_start = chunk_end;
    }
    matchers
}

fn compile_matcher_chunk(
    entries: &[DictEntry],
    entry_indices: &[usize],
    regex_size_limit: Option<usize>,
    matchers: &mut Vec<PhraseMatcher>,
) {
    let mut pattern = String::from(r"\A(?:");
    for (position, entry_index) in entry_indices.iter().enumerate() {
        if position > 0 {
            pattern.push('|');
        }
        pattern.push('(');
        pattern.push_str(&regex::escape(&canonical_fold(
            &entries[*entry_index].spoken,
        )));
        pattern.push_str(r")(?:$|[^\w'])");
    }
    pattern.push(')');

    if let Ok(regex) = compile_phrase_regex(&pattern, regex_size_limit) {
        matchers.push(PhraseMatcher::Regex {
            regex,
            entry_indices: entry_indices.to_vec(),
        });
    } else if entry_indices.len() > 1 {
        let middle = entry_indices.len() / 2;
        compile_matcher_chunk(
            entries,
            &entry_indices[..middle],
            regex_size_limit,
            matchers,
        );
        compile_matcher_chunk(
            entries,
            &entry_indices[middle..],
            regex_size_limit,
            matchers,
        );
    } else {
        let entry_index = entry_indices[0];
        let spoken = canonical_fold(&entries[entry_index].spoken);
        if let Ok(regex) = compile_phrase_regex(&regex::escape(&spoken), None) {
            matchers.push(PhraseMatcher::SingleRegex { regex, entry_index });
        } else {
            matchers.push(PhraseMatcher::Literal {
                entry_index,
                needle: spoken,
            });
        }
    }
}

fn compile_phrase_regex(pattern: &str, size_limit: Option<usize>) -> Result<Regex, regex::Error> {
    let mut builder = RegexBuilder::new(pattern);
    if let Some(size_limit) = size_limit {
        builder.size_limit(size_limit);
    }
    builder.build()
}

fn matcher_candidates(matcher: &PhraseMatcher, text: &str) -> Vec<(usize, usize, usize)> {
    match matcher {
        PhraseMatcher::Regex { regex, .. } => text
            .char_indices()
            .filter_map(|(start, _)| {
                let captures = regex.captures(&text[start..])?;
                let (priority, matched) = captures
                    .iter()
                    .skip(1)
                    .enumerate()
                    .find_map(|(priority, capture)| capture.map(|matched| (priority, matched)))?;
                let end = start + matched.end();
                Some((priority, start, end))
            })
            .collect(),
        PhraseMatcher::SingleRegex { regex, .. } => regex
            .find_iter(text)
            .map(|matched| (0, matched.start(), matched.end()))
            .collect(),
        PhraseMatcher::Literal { needle, .. } => text
            .match_indices(needle)
            .map(|(start, matched)| (0, start, start + matched.len()))
            .collect(),
    }
}

fn matcher_entry_index(matcher: &PhraseMatcher, priority: usize) -> usize {
    match matcher {
        PhraseMatcher::Regex { entry_indices, .. } => entry_indices[priority],
        PhraseMatcher::SingleRegex { entry_index, .. }
        | PhraseMatcher::Literal { entry_index, .. } => *entry_index,
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
    static WORD_CHAR: OnceLock<Regex> = OnceLock::new();

    if c == '\'' {
        return true;
    }
    let mut encoded = [0; 4];
    WORD_CHAR
        .get_or_init(|| Regex::new(r"^\w$").expect("Unicode word regex must compile"))
        .is_match(c.encode_utf8(&mut encoded))
}

fn clean_phrase(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn phrase_key(value: &str) -> String {
    canonical_fold(&clean_phrase(value))
}

fn canonical_fold(value: &str) -> String {
    value.chars().nfd().default_case_fold().nfd().collect()
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
    use crate::paths::{fail_next_private_write, PrivateWriteFailure};
    use std::io::Read;
    use std::net::TcpListener;
    use std::process::{Command, Stdio};

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
        let root = std::env::temp_dir().join(format!(
            "echo-dictionary-batch-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let mut dictionary = dict(entries);
        dictionary.path = root.join("dictionary.json");
        dictionary.save().unwrap();
        dictionary
    }

    fn assert_no_dictionary_temp_file(path: &Path) {
        let prefix = format!(".{}.tmp-", path.file_name().unwrap().to_string_lossy());
        let residual = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .find(|name| name.to_string_lossy().starts_with(&prefix));
        assert!(
            residual.is_none(),
            "residual dictionary temp file: {residual:?}"
        );
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
    fn rewrites_accented_text_with_unicode_case_folding() {
        let d = dict(&[("école", "school")]);
        assert_eq!(d.rewrite("ÉCOLE et école"), "school et school");
    }

    #[test]
    fn canonical_equivalents_match_in_both_directions() {
        let composed = dict(&[("école", "school")]);
        assert_eq!(composed.rewrite("e\u{301}cole"), "school");

        let decomposed = dict(&[("e\u{301}cole", "school")]);
        assert_eq!(decomposed.rewrite("ÉCOLE"), "school");
    }

    #[test]
    fn full_case_folds_match_only_at_original_grapheme_boundaries() {
        let word = dict(&[("straße", "street")]);
        assert_eq!(word.rewrite("STRASSE und Straße"), "street und street");

        let letter = dict(&[("s", "S")]);
        assert_eq!(letter.rewrite("ß s"), "ß S");
    }

    #[test]
    fn rewrites_greek_and_cyrillic_with_unicode_case_folding() {
        let d = dict(&[("αθήνα", "Athens"), ("москва", "Moscow")]);
        assert_eq!(d.rewrite("ΑΘΉΝΑ и МОСКВА"), "Athens и Moscow");
    }

    #[test]
    fn rewrites_another_non_latin_script_as_a_whole_phrase() {
        let d = dict(&[("東京", "Tokyo")]);
        assert_eq!(d.rewrite("東京、東京都"), "Tokyo、東京都");
    }

    #[test]
    fn combining_marks_are_matched_and_are_unicode_word_characters() {
        let decomposed = dict(&[("cafe\u{301}", "CAFÉ")]);
        assert_eq!(decomposed.rewrite("CAFE\u{301}"), "CAFÉ");

        let base = dict(&[("cafe", "CAFE")]);
        assert_eq!(base.rewrite("cafe\u{301} cafe"), "cafe\u{301} CAFE");
    }

    #[test]
    fn does_not_match_inside_unicode_words() {
        let d = dict(&[("code", "CODE"), ("京", "capital")]);
        assert_eq!(d.rewrite("πcodeδ code 東京 京"), "πcodeδ CODE 東京 capital");
    }

    #[test]
    fn apostrophes_keep_the_existing_word_boundary_semantics() {
        let base = dict(&[("can", "CAN"), ("rock", "ROCK")]);
        assert_eq!(
            base.rewrite("can't can rock'n'roll rock"),
            "can't CAN rock'n'roll ROCK"
        );

        let apostrophe_phrase = dict(&[("can't", "cannot")]);
        assert_eq!(apostrophe_phrase.rewrite("CAN'T stop"), "cannot stop");
    }

    #[test]
    fn rejected_multibyte_candidates_do_not_break_later_byte_offsets() {
        let d = dict(&[("clair", "CLEAR")]);
        assert_eq!(
            d.rewrite("éclair, 🙂 CLAIR après"),
            "éclair, 🙂 CLEAR après"
        );
    }

    #[test]
    fn regex_metacharacters_in_phrases_are_literals() {
        let d = dict(&[("c++", "C Plus Plus"), ("a.b", "dot"), ("[tag]", "label")]);
        assert_eq!(
            d.rewrite("c++ a.b [tag] acb xa.by"),
            "C Plus Plus dot label acb xa.by"
        );
    }

    #[test]
    fn longer_overlapping_phrase_wins_independent_of_entry_order() {
        let text = "alpha beta gamma delta";
        let expected = "alpha LONG";
        assert_eq!(
            dict(&[("alpha beta", "SHORT"), ("beta gamma delta", "LONG")]).rewrite(text),
            expected
        );
        assert_eq!(
            dict(&[("beta gamma delta", "LONG"), ("alpha beta", "SHORT")]).rewrite(text),
            expected
        );
    }

    #[test]
    fn equal_length_overlaps_follow_dictionary_order_deterministically() {
        let text = "東京 alpha βγ";
        let mut first_before_second = dict(&[("東京 alpha", "FIRST"), ("alpha βγ", "SECOND")]);
        first_before_second.entries[0].created_at = 100;
        first_before_second.entries[1].created_at = 1;
        assert_eq!(first_before_second.rewrite(text), "FIRST βγ");

        let mut second_before_first = dict(&[("alpha βγ", "SECOND"), ("東京 alpha", "FIRST")]);
        second_before_first.entries[0].created_at = 200;
        second_before_first.entries[1].created_at = 2;
        assert_eq!(second_before_first.rewrite(text), "東京 SECOND");
    }

    #[test]
    fn compilation_fallback_preserves_all_matches_and_global_ranking() {
        let d = dict(&[
            ("beta gamma", "FIRST"),
            ("alpha beta", "SECOND"),
            ("beta gamma delta", "LONG"),
        ]);

        let rewritten = d.rewrite_with_limits(
            "alpha beta gamma delta | alpha beta gamma | beta gamma | alpha beta",
            MatchLimits {
                entries: 2,
                pattern_bytes: usize::MAX,
                regex_size_limit: Some(0),
            },
        );

        assert_eq!(rewritten, "alpha LONG | alpha FIRST | FIRST | SECOND");
    }

    #[test]
    fn invalid_longer_boundary_does_not_hide_a_valid_shorter_alternative() {
        let d = dict(&[("alpha.", "LONG"), ("alpha", "SHORT")]);
        assert_eq!(d.rewrite("alpha.beta"), "SHORT.beta");
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
        let _ = fs::remove_dir_all(dictionary.path.parent().unwrap());
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
        let _ = fs::remove_dir_all(dictionary.path.parent().unwrap());
    }

    #[test]
    fn batch_write_failure_keeps_memory_and_persisted_entries_unchanged() {
        for failure in [PrivateWriteFailure::Write, PrivateWriteFailure::Sync] {
            let mut dictionary = persisted_dict(&[("existing", "Existing")]);
            let before_entries = dictionary.entries().to_vec();
            let before_file = fs::read(&dictionary.path).unwrap();

            fail_next_private_write(failure);
            let result = dictionary.add_batch("Canonical", ["new hearing".to_string()]);

            assert!(result.is_err());
            assert_eq!(dictionary.entries(), before_entries);
            assert_eq!(fs::read(&dictionary.path).unwrap(), before_file);
            let _ = fs::remove_dir_all(dictionary.path.parent().unwrap());
        }
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
    fn stale_save_reloads_cross_process_state_instead_of_clobbering_it() {
        let mut current = persisted_dict(&[("existing", "Existing")]);
        let stale = Dictionary::load_from(&current.path).unwrap();
        current.add("new hearing", "Canonical").unwrap();

        stale.save().unwrap();

        let persisted = Dictionary::load_from(&current.path).unwrap();
        assert_eq!(persisted.entries(), current.entries());
        let _ = fs::remove_dir_all(current.path.parent().unwrap());
    }

    #[test]
    fn failed_save_keeps_persisted_entries_unchanged_without_temp_files() {
        for failure in [PrivateWriteFailure::Write, PrivateWriteFailure::Sync] {
            let dictionary = persisted_dict(&[("existing", "Existing")]);
            let before_entries = dictionary.entries().to_vec();
            let before_file = fs::read(&dictionary.path).unwrap();

            fail_next_private_write(failure);
            let error = dictionary.save().unwrap_err();

            assert!(error.contains("injected private"), "{error}");
            assert_eq!(dictionary.entries(), before_entries);
            assert_eq!(fs::read(&dictionary.path).unwrap(), before_file);
            assert_no_dictionary_temp_file(&dictionary.path);
            let _ = fs::remove_dir_all(dictionary.path.parent().unwrap());
        }
    }

    #[test]
    fn add_write_failure_keeps_memory_and_persisted_entries_unchanged() {
        for failure in [PrivateWriteFailure::Write, PrivateWriteFailure::Sync] {
            let mut dictionary = persisted_dict(&[("existing", "Existing")]);
            let before_entries = dictionary.entries().to_vec();
            let before_file = fs::read(&dictionary.path).unwrap();

            fail_next_private_write(failure);
            assert!(dictionary.add("new hearing", "Canonical").is_err());

            assert_eq!(dictionary.entries(), before_entries);
            assert_eq!(fs::read(&dictionary.path).unwrap(), before_file);
            let _ = fs::remove_dir_all(dictionary.path.parent().unwrap());
        }
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
    fn remove_write_failure_keeps_memory_and_persisted_entries_unchanged() {
        for failure in [PrivateWriteFailure::Write, PrivateWriteFailure::Sync] {
            let mut dictionary =
                persisted_dict(&[("clawed code", "Claude Code"), ("echo", "Echo")]);
            let before_entries = dictionary.entries().to_vec();
            let before_file = fs::read(&dictionary.path).unwrap();

            fail_next_private_write(failure);
            assert!(dictionary.remove("clawed code", "Claude Code").is_err());

            assert_eq!(dictionary.entries(), before_entries);
            assert_eq!(fs::read(&dictionary.path).unwrap(), before_file);
            let _ = fs::remove_dir_all(dictionary.path.parent().unwrap());
        }
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
    fn read_only_corrupt_dictionary_is_reported_without_modifying_it() {
        let dir = std::env::temp_dir().join(format!(
            "echo-dict-read-only-corrupt-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dictionary.json");
        let original = b"{\"entries\":[";
        fs::write(&path, original).unwrap();

        let error = Dictionary::load_from_read_only(&path).unwrap_err();

        assert!(error.contains("invalid JSON"), "{error}");
        assert!(error.contains(path.to_str().unwrap()), "{error}");
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(!path.with_extension("json.corrupt").exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn empty_object_loads_as_empty_dictionary_without_quarantine() {
        let dir = std::env::temp_dir().join(format!("echo-dict-empty-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dictionary.json");
        fs::write(&path, "{}").unwrap();

        assert!(Dictionary::load_from(&path).unwrap().entries().is_empty());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{}");
        assert!(!dir.join("dictionary.json.corrupt").exists());
    }

    #[test]
    fn nonempty_object_without_entries_is_still_rejected() {
        let dir = std::env::temp_dir().join(format!(
            "echo-dict-incompatible-empty-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dictionary.json");
        fs::write(&path, r#"{"unexpected":true}"#).unwrap();

        assert!(Dictionary::load_from(&path).is_err());
        assert!(!path.exists());
        assert!(dir.join("dictionary.json.corrupt").exists());
    }

    #[cfg(unix)]
    #[test]
    fn dictionary_lock_secures_its_directory_and_file() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!(
            "echo-dictionary-private-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let path = dir.join("dictionary.json");
        let lock = lock_dictionary_file(&path).unwrap();

        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(dir.join("dictionary.json.lock"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );

        drop(lock);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn cross_process_update_helper() {
        let Some(path) = std::env::var_os("ECHO_TEST_DICTIONARY_CHILD_PATH") else {
            return;
        };
        let mut dictionary = Dictionary::load_from(PathBuf::from(path)).unwrap();
        dictionary.add("child sound", "Child").unwrap();
    }

    #[test]
    fn concurrent_process_updates_reload_locked_state_and_preserve_both_changes() {
        let dir = std::env::temp_dir().join(format!(
            "echo-dict-cross-process-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dictionary.json");
        Dictionary::load_from(&path).unwrap().save().unwrap();

        let lock_file = lock_dictionary_file(&path).unwrap();
        let contention_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let contention_address = contention_listener.local_addr().unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .arg("--exact")
            .arg("dictionary::tests::cross_process_update_helper")
            .env("ECHO_TEST_DICTIONARY_CHILD_PATH", &path)
            .env(
                "ECHO_TEST_DICTIONARY_CONTENTION_ADDRESS",
                contention_address.to_string(),
            )
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();

        let (mut contention, _) = contention_listener.accept().unwrap();
        let mut marker = [0_u8; 25];
        contention.read_exact(&mut marker).unwrap();
        assert_eq!(&marker, b"dictionary lock contended");
        assert!(
            Dictionary::load_from_read_only(&path)
                .unwrap()
                .entries()
                .is_empty(),
            "child changed the persisted dictionary while the parent held the advisory lock"
        );
        assert!(
            child.try_wait().unwrap().is_none(),
            "child update was not blocked by the advisory lock"
        );

        let mut parent = Dictionary::load_from(&path).unwrap();
        parent.entries.push(DictEntry {
            spoken: "parent sound".to_string(),
            written: "Parent".to_string(),
            created_at: 1,
        });
        parent.save_entries(&parent.entries).unwrap();
        FileExt::unlock(&lock_file).unwrap();
        assert!(child.wait().unwrap().success());
        let persisted = Dictionary::load_from(&path).unwrap();
        assert_eq!(persisted.entries().len(), 2);
        assert!(persisted
            .entries()
            .iter()
            .any(|entry| entry.spoken == "parent sound" && entry.written == "Parent"));
        assert!(persisted
            .entries()
            .iter()
            .any(|entry| entry.spoken == "child sound" && entry.written == "Child"));
    }

    #[test]
    fn corrupt_file_is_set_aside_and_reported() {
        let dir = std::env::temp_dir().join(format!("echo-dict-corrupt-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dictionary.json");
        let original = "not json at all";
        fs::write(&path, original).unwrap();

        let error = Dictionary::load_from(&path).unwrap_err();
        assert!(error.contains("invalid JSON"), "{error}");
        assert!(error.contains(path.to_str().unwrap()), "{error}");
        assert!(!path.exists(), "corrupt file should be moved aside");
        assert_eq!(
            fs::read_to_string(dir.join("dictionary.json.corrupt")).unwrap(),
            original
        );
    }
}
