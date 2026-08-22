use serde::{Deserialize, Serialize};

use crate::types::{EngineId, Pcm16kMono};

/// What actually ran on a transcription, observed from the engine rather than
/// requested in configuration. Every field is optional: Parakeet has no model
/// file or multilingual flag to report, and the fake engine has nothing at all.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RunDetail {
    #[serde(default)]
    pub binary: Option<String>,
    #[serde(default)]
    pub model_path: Option<String>,
    #[serde(default)]
    pub multilingual: Option<bool>,
    #[serde(default)]
    pub vad: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transcript {
    pub raw: String,
    pub engine: EngineId,
    pub language: Option<String>,
    pub audio_ms: u64,
    pub infer_ms: u64,
    pub detail: RunDetail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineError {
    Missing,
    Infer(String),
}

impl EngineError {
    #[must_use]
    pub fn as_str(&self) -> String {
        match self {
            Self::Missing => "engine or model missing".to_string(),
            Self::Infer(msg) => msg.clone(),
        }
    }
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.as_str())
    }
}

impl std::error::Error for EngineError {}

pub trait Engine {
    fn id(&self) -> EngineId;
    fn transcribe(&self, pcm: &Pcm16kMono) -> Result<Transcript, EngineError>;
}
