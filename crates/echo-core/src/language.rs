use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::language_table::LANGUAGE_TABLE;

/// A language whisper.cpp knows, as a branded code. Constructible only from
/// the generated table, so an invalid code can never reach the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Language(&'static str);

impl Language {
    pub const ENGLISH: Self = Self("en");

    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        LANGUAGE_TABLE
            .iter()
            .find(|(known, _, _)| *known == code)
            .map(|(known, _, _)| Self(known))
    }

    #[must_use]
    pub fn code(self) -> &'static str {
        self.0
    }

    #[must_use]
    pub fn id(self) -> u16 {
        LANGUAGE_TABLE
            .iter()
            .find(|(known, _, _)| *known == self.0)
            .map(|(_, id, _)| *id)
            .unwrap_or(0)
    }

    #[must_use]
    pub fn english_name(self) -> &'static str {
        LANGUAGE_TABLE
            .iter()
            .find(|(known, _, _)| *known == self.0)
            .map(|(_, _, name)| *name)
            .unwrap_or(self.0)
    }

    /// Every language whisper.cpp supports, in table (id) order.
    pub fn all() -> impl Iterator<Item = Self> + 'static {
        LANGUAGE_TABLE.iter().map(|(code, _, _)| Self(code))
    }
}

/// The language Echo transcribes in. Two explicit states: `Option<Language>`
/// would conflate "auto" with "unset".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LanguageChoice {
    /// Pass `-l auto`; whisper.cpp runs detection on the first 30-second
    /// window and the result applies to the whole file. Costs one extra
    /// encoder pass over a pinned language.
    Auto,
    Pinned(Language),
}

impl Default for LanguageChoice {
    /// Pinned English matches what Whisper did before Echo had a language
    /// concept, and detection is an explicit opt-in because it costs latency.
    fn default() -> Self {
        Self::Pinned(Language::ENGLISH)
    }
}

impl LanguageChoice {
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "auto" => Some(Self::Auto),
            code => Language::from_code(code).map(Self::Pinned),
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Pinned(language) => language.code(),
        }
    }

    /// Whether the English cleanup rules (filler dropping, ASCII punctuation)
    /// may run for a session. Pinned English allows them, a pinned other
    /// language forbids them, and under auto they run only when detection
    /// reported English. An absent observation stays conservative because
    /// engines such as Parakeet cannot report the detected language.
    #[must_use]
    pub fn permits_english_rules(&self, detected: Option<&str>) -> bool {
        match self {
            Self::Pinned(language) => *language == Language::ENGLISH,
            Self::Auto => detected == Some("en"),
        }
    }
}

impl Serialize for LanguageChoice {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for LanguageChoice {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).ok_or_else(|| serde::de::Error::custom(format!("unknown language {raw}")))
    }
}

/// Parakeet-TDT 0.6B v3's fixed capability: 25 European languages with
/// automatic identification and no readback, per the sherpa-onnx model card.
/// There is no language flag to pass, so there is no picker for it.
pub const PARAKEET_LANGUAGES: &[&str] = &[
    "bg", "hr", "cs", "da", "nl", "en", "et", "fi", "fr", "de", "el", "hu", "it", "lv", "lt", "mt",
    "pl", "pt", "ro", "sk", "sl", "es", "sv", "ru", "uk",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_has_100_entries_en_first_yue_last() {
        assert_eq!(Language::all().count(), 100);
        assert_eq!(Language::ENGLISH.id(), 0);
        let yue = Language::from_code("yue").unwrap();
        assert_eq!(yue.id(), 99);
        assert_eq!(yue.english_name(), "cantonese");
    }

    #[test]
    fn unknown_code_is_rejected_at_construction() {
        assert_eq!(Language::from_code("xx"), None);
        assert_eq!(Language::from_code(""), None);
        assert_eq!(Language::from_code("EN"), None);
    }

    #[test]
    fn choice_parses_auto_and_codes() {
        assert_eq!(LanguageChoice::parse("auto"), Some(LanguageChoice::Auto));
        assert_eq!(
            LanguageChoice::parse("de"),
            Some(LanguageChoice::Pinned(Language::from_code("de").unwrap()))
        );
        assert_eq!(LanguageChoice::parse("klingon"), None);
    }

    #[test]
    fn choice_defaults_to_pinned_english() {
        assert_eq!(
            LanguageChoice::default(),
            LanguageChoice::Pinned(Language::ENGLISH)
        );
    }

    #[test]
    fn choice_round_trips_through_json() {
        for choice in [LanguageChoice::Auto, LanguageChoice::default()] {
            let raw = serde_json::to_string(&choice).unwrap();
            assert_eq!(
                serde_json::from_str::<LanguageChoice>(&raw).unwrap(),
                choice
            );
        }
        assert_eq!(
            serde_json::to_string(&LanguageChoice::Auto).unwrap(),
            "\"auto\""
        );
        assert!(serde_json::from_str::<LanguageChoice>("\"xx\"").is_err());
    }

    #[test]
    fn english_rules_follow_the_resolved_language() {
        let german = LanguageChoice::Pinned(Language::from_code("de").unwrap());
        assert!(!german.permits_english_rules(None));
        assert!(LanguageChoice::default().permits_english_rules(None));
        assert!(LanguageChoice::Auto.permits_english_rules(Some("en")));
        assert!(!LanguageChoice::Auto.permits_english_rules(Some("ja")));
        assert!(!LanguageChoice::Auto.permits_english_rules(None));
    }

    #[test]
    fn parakeet_capability_is_25_languages() {
        assert_eq!(PARAKEET_LANGUAGES.len(), 25);
        assert!(PARAKEET_LANGUAGES.contains(&"en"));
    }
}
