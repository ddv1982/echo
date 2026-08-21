use crate::types::{EngineId, Pcm16kMono};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transcript {
    pub raw: String,
    pub engine: EngineId,
    pub language: Option<String>,
    pub audio_ms: u64,
    pub infer_ms: u64,
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
