use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::dictionary::{Dictionary, Rewrite};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CleanupMode {
    Off,
    Rules,
    LocalModel { model: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CleanupError {
    InvalidMode(String),
    Local(String),
}

impl std::fmt::Display for CleanupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMode(msg) | Self::Local(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for CleanupError {}

impl CleanupMode {
    pub fn parse(raw: &str) -> Result<Self, CleanupError> {
        let trimmed = raw.trim();
        let lower = trimmed.to_ascii_lowercase();
        if lower.contains("://") || lower.starts_with("http") {
            return Err(CleanupError::InvalidMode(
                "cloud cleanup URLs are not allowed".to_string(),
            ));
        }
        match lower.as_str() {
            "off" => Ok(Self::Off),
            "rules" | "" => Ok(Self::Rules),
            other if other.starts_with("local:") => Ok(Self::LocalModel {
                model: trimmed[6..].to_string(),
            }),
            "local" => Ok(Self::LocalModel {
                model: "echo-cleanup".to_string(),
            }),
            other => Err(CleanupError::InvalidMode(format!(
                "unknown cleanup mode {other}"
            ))),
        }
    }

    fn as_file_str(&self) -> String {
        match self {
            Self::Off => "off".to_string(),
            Self::Rules => "rules".to_string(),
            Self::LocalModel { model } => format!("local:{model}"),
        }
    }
}

impl Serialize for CleanupMode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.as_file_str())
    }
}

impl<'de> Deserialize<'de> for CleanupMode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(serde::de::Error::custom)
    }
}

pub trait Cleanup {
    fn apply(&self, raw: &str, dict: &Dictionary) -> Result<Rewrite, CleanupError>;
}

#[derive(Debug, Default, Clone)]
pub struct OffCleanup;

impl Cleanup for OffCleanup {
    fn apply(&self, raw: &str, dict: &Dictionary) -> Result<Rewrite, CleanupError> {
        Ok(dict.rewrite(raw))
    }
}

#[derive(Debug, Default, Clone)]
pub struct RulesCleanup;

impl Cleanup for RulesCleanup {
    fn apply(&self, raw: &str, dict: &Dictionary) -> Result<Rewrite, CleanupError> {
        let cleaned = punctuate(&capitalize(&drop_fillers(raw)));
        Ok(dict.rewrite(&cleaned))
    }
}

fn drop_fillers(raw: &str) -> String {
    raw.split_whitespace()
        .filter(|tok| !is_filler(tok))
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_filler(token: &str) -> bool {
    let letters: String = token
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .collect::<String>()
        .to_ascii_lowercase();
    matches!(letters.as_str(), "um" | "uh")
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => {
            let mut out = String::new();
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
            out
        }
        None => String::new(),
    }
}

fn punctuate(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    match trimmed.chars().last() {
        Some('.' | '!' | '?') => trimmed.to_string(),
        _ => format!("{trimmed}."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dictionary::Dictionary;

    fn empty_dict() -> Dictionary {
        Dictionary::empty()
    }

    #[test]
    fn rules_clean_spoken_ramble() {
        let rewrite = RulesCleanup
            .apply("um so can we uh move the button", &empty_dict())
            .unwrap();
        assert_eq!(rewrite.text, "So can we move the button.");
    }

    #[test]
    fn rules_keep_like_as_a_content_word() {
        let rewrite = RulesCleanup
            .apply("i like this approach", &empty_dict())
            .unwrap();
        assert_eq!(rewrite.text, "I like this approach.");
    }

    #[test]
    fn off_keeps_raw_then_dict() {
        let rewrite = OffCleanup
            .apply("um so like can we uh move the button", &empty_dict())
            .unwrap();
        assert_eq!(rewrite.text, "um so like can we uh move the button");
    }

    #[test]
    fn dict_hits_after_rules() {
        let dir = std::env::temp_dir().join(format!("echo-clean-dict-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let mut dict = Dictionary::load_from(dir.join("dictionary.json")).unwrap();
        dict.add("button", "Button").unwrap();
        let rewrite = RulesCleanup
            .apply("um so can we uh move the button", &dict)
            .unwrap();
        assert_eq!(rewrite.text, "So can we move the Button.");
    }

    #[test]
    fn rejects_cloud_url() {
        assert!(CleanupMode::parse("https://example.com/clean").is_err());
    }

    #[test]
    fn parses_modes() {
        assert_eq!(CleanupMode::parse("off").unwrap(), CleanupMode::Off);
        assert_eq!(CleanupMode::parse("rules").unwrap(), CleanupMode::Rules);
        assert_eq!(
            CleanupMode::parse("local:llama3"),
            Ok(CleanupMode::LocalModel {
                model: "llama3".to_string()
            })
        );
    }
}
