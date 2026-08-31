mod config;
mod dictionary;
mod engine;
mod history;
mod inject;
mod language;
mod language_table;
mod nonspeech;
mod paths;
mod recording;
mod session;
mod types;

pub use config::{resolve, Config, EngineChoice, MicrophoneSelection};
pub use dictionary::{DictEntry, Dictionary, RecognitionHints};
pub use engine::{
    DecodeOptions, Engine, EngineError, RunDetail, Transcript, WhisperAccelerationPreference,
    WhisperAccelerationSkip, WhisperAttemptTelemetry, WhisperRecoveryReason,
    WhisperRecoveryTelemetry, WhisperRetryReason, WhisperRunMode, WhisperRunTelemetry,
    WhisperRuntimeBackend, WhisperRuntimeSource, WhisperRuntimeTelemetry, WhisperTuningTelemetry,
    WhisperVulkanReceipt,
};
pub use history::{History, HistoryRow};
pub use inject::{FocusTarget, InjectBackend, InjectReport, Injector};
pub use language::{Language, LanguageChoice, PARAKEET_LANGUAGES};
pub use nonspeech::strip_nonspeech;
pub use paths::{
    config_dir, config_path, data_dir, dictionary_path, history_path, status_path, write_atomic,
};
pub use recording::{
    resolve_recording_limit, RecordingLimit, RecordingLimitSource, ResolvedRecordingLimit,
};
pub use session::{Session, SessionError, SessionState};
pub use types::{EngineId, FailReason, Pcm16kMono, SAMPLE_RATE_HZ};

#[cfg(test)]
mod types_tests {
    use super::*;

    #[test]
    fn pcm_duration_and_empty_rms() {
        let empty = Pcm16kMono::from_samples(Vec::new());
        assert_eq!(empty.duration_ms(), 0);
        assert!(empty.peak_rms() == 0.0);

        let one_second = Pcm16kMono::from_samples(vec![0; SAMPLE_RATE_HZ as usize]);
        assert_eq!(one_second.duration_ms(), 1000);
        assert!(one_second.peak_rms() == 0.0);
    }
}
