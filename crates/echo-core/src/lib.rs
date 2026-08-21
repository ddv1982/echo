mod engine;
mod inject;
mod session;
mod types;

pub use engine::{Engine, EngineError, Transcript};
pub use inject::{FocusTarget, InjectBackend, InjectReport, Injector};
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
