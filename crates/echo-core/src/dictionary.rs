use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

use crate::paths::{dictionary_path, set_aside_corrupt, write_atomic_private};

const MATCH_CHUNK_ENTRIES: usize = 64;
const MATCH_CHUNK_PATTERN_BYTES: usize = 16 * 1024;

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
        lowercase: String,
        uppercase: String,
    },
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
            Err(error) => {
                set_aside_corrupt(&path);
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
        if !path.exists() {
            return Ok(Self {
                entries: Vec::new(),
                path,
            });
        }
        let raw = fs::read_to_string(&path).map_err(|err| err.to_string())?;
        let entries = serde_json::from_str::<DictFile>(&raw)
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
        write_atomic_private(&self.path, raw.as_bytes())
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
            let mut candidates = matcher_candidates(&matcher, text);
            candidates.sort_by_key(|(priority, start, _)| (*priority, *start));
            for (priority, start, end) in candidates {
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
            let escaped_bytes = regex::escape(&entries[ranked_indices[chunk_end]].spoken).len();
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
        pattern.push_str(&regex::escape(&entries[*entry_index].spoken));
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
        let spoken = &entries[entry_index].spoken;
        if let Ok(regex) = compile_phrase_regex(&regex::escape(spoken), None) {
            matchers.push(PhraseMatcher::SingleRegex { regex, entry_index });
        } else {
            matchers.push(PhraseMatcher::Literal {
                entry_index,
                lowercase: spoken.chars().flat_map(char::to_lowercase).collect(),
                uppercase: spoken.chars().flat_map(char::to_uppercase).collect(),
            });
        }
    }
}

fn compile_phrase_regex(pattern: &str, size_limit: Option<usize>) -> Result<Regex, regex::Error> {
    let mut builder = RegexBuilder::new(pattern);
    builder.case_insensitive(true);
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
        PhraseMatcher::Literal {
            lowercase,
            uppercase,
            ..
        } => text
            .char_indices()
            .filter_map(|(start, _)| {
                folded_literal_end(text, start, lowercase, char::to_lowercase)
                    .or_else(|| folded_literal_end(text, start, uppercase, char::to_uppercase))
                    .map(|end| (0, start, end))
            })
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

fn folded_literal_end<I>(
    text: &str,
    start: usize,
    folded_needle: &str,
    fold: impl Fn(char) -> I,
) -> Option<usize>
where
    I: Iterator<Item = char>,
{
    let mut folded = String::new();
    for (offset, character) in text[start..].char_indices() {
        folded.extend(fold(character));
        if folded == folded_needle {
            return Some(start + offset + character.len_utf8());
        }
        if !folded_needle.starts_with(&folded) {
            return None;
        }
    }
    None
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
        let _ = fs::remove_file(&dictionary.path);
        let _ = fs::remove_dir(dictionary.path.parent().unwrap());
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
        let _ = fs::remove_dir(dictionary.path.parent().unwrap());
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
    fn read_only_corrupt_dictionary_is_reported_without_modifying_it() {
        let path = std::env::temp_dir().join(format!(
            "echo-dict-read-only-corrupt-{}.json",
            std::process::id()
        ));
        let original = b"{\"entries\":[";
        fs::write(&path, original).unwrap();

        let error = Dictionary::load_from_read_only(&path).unwrap_err();

        assert!(error.contains("invalid JSON"), "{error}");
        assert!(error.contains(path.to_str().unwrap()), "{error}");
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(!path.with_extension("json.corrupt").exists());
        let _ = fs::remove_file(path);
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
