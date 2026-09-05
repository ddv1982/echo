use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use echo::audio::{AudioCapture, CancellationToken, CaptureResult};
use echo::transcribe::{RunOverrides, TranscriptionPurpose};
use echo_desktop::ipc::DictionaryTrainingSample;
use tauri::State;

const MAX_SAMPLE_DURATION: Duration = Duration::from_secs(30);
static NEXT_CAPTURE_ID: AtomicU64 = AtomicU64::new(1);

struct ActiveCapture {
    cancel: CancellationToken,
    recording: Option<JoinHandle<Result<TrainingCapture, echo::audio::AudioError>>>,
}

struct TrainingCapture {
    audio: CaptureResult,
    session: echo::rec::RecordingSession,
}

enum CapturePhase<T> {
    Starting { id: String, cancelled: bool },
    Active { id: String, value: T },
}

struct CaptureStartupState<T> {
    phase: Mutex<Option<CapturePhase<T>>>,
}

impl<T> Default for CaptureStartupState<T> {
    fn default() -> Self {
        Self {
            phase: Mutex::new(None),
        }
    }
}

impl<T> CaptureStartupState<T> {
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Option<CapturePhase<T>>>, String> {
        self.phase
            .lock()
            .map_err(|_| "Voice training capture is unavailable.".to_string())
    }

    fn start_with(
        &self,
        capture_id: String,
        start: impl FnOnce() -> Result<T, String>,
    ) -> Result<bool, String> {
        {
            let mut phase = self.lock()?;
            if phase.is_some() {
                return Err("A voice training sample is already recording.".to_string());
            }
            *phase = Some(CapturePhase::Starting {
                id: capture_id.clone(),
                cancelled: false,
            });
        }

        let mut value = match start() {
            Ok(value) => Some(value),
            Err(error) => {
                let mut phase = self.lock()?;
                if matches!(
                    phase.as_ref(),
                    Some(CapturePhase::Starting { id, .. }) if id == &capture_id
                ) {
                    *phase = None;
                }
                return Err(error);
            }
        };

        let published = {
            let mut phase = self.lock()?;
            match phase.as_ref() {
                Some(CapturePhase::Starting {
                    id,
                    cancelled: false,
                }) if id == &capture_id => {
                    *phase = Some(CapturePhase::Active {
                        id: capture_id,
                        value: value.take().expect("opened capture"),
                    });
                    true
                }
                Some(CapturePhase::Starting { id, .. }) if id == &capture_id => {
                    *phase = None;
                    false
                }
                _ => false,
            }
        };
        drop(value);
        Ok(published)
    }

    fn take_active(&self, capture_id: &str) -> Result<Option<T>, String> {
        let mut phase = self.lock()?;
        if matches!(
            phase.as_ref(),
            Some(CapturePhase::Active { id, .. }) if id == capture_id
        ) {
            let Some(CapturePhase::Active { value, .. }) = phase.take() else {
                unreachable!("matched active capture")
            };
            Ok(Some(value))
        } else {
            Ok(None)
        }
    }

    fn cancel(&self, capture_id: &str) -> Result<CaptureCancellation<T>, String> {
        let mut phase = self.lock()?;
        if let Some(CapturePhase::Starting { id, cancelled }) = phase.as_mut() {
            if id == capture_id {
                *cancelled = true;
                return Ok(CaptureCancellation::Starting);
            }
        }
        if matches!(
            phase.as_ref(),
            Some(CapturePhase::Active { id, .. }) if id == capture_id
        ) {
            let Some(CapturePhase::Active { value, .. }) = phase.take() else {
                unreachable!("matched active capture")
            };
            Ok(CaptureCancellation::Active(value))
        } else {
            Ok(CaptureCancellation::Missing)
        }
    }
}

enum CaptureCancellation<T> {
    Missing,
    Starting,
    Active(T),
}

impl<T> CaptureCancellation<T> {
    fn accepted(&self) -> bool {
        !matches!(self, Self::Missing)
    }

    fn into_active(self) -> Option<T> {
        match self {
            Self::Active(active) => Some(active),
            Self::Missing | Self::Starting => None,
        }
    }
}

impl ActiveCapture {
    fn finish(mut self) -> Result<TrainingCapture, String> {
        self.cancel.cancel();
        self.recording
            .take()
            .expect("active capture recording thread")
            .join()
            .map_err(|_| "Voice training capture stopped unexpectedly.".to_string())?
            .map_err(|error| error.to_string())
    }
}

impl Drop for ActiveCapture {
    fn drop(&mut self) {
        self.cancel.cancel();
        if let Some(recording) = self.recording.take() {
            let _ = recording.join();
        }
    }
}

#[derive(Default)]
pub(crate) struct DictionaryTrainingCaptures {
    state: Arc<CaptureStartupState<ActiveCapture>>,
}

#[tauri::command]
pub(crate) async fn start_dictionary_training_sample(
    captures: State<'_, DictionaryTrainingCaptures>,
) -> Result<String, String> {
    let state = Arc::clone(&captures.state);
    crate::blocking::run_blocking("voice training capture start", move || {
        let capture_id = format!(
            "{}-{}",
            std::process::id(),
            NEXT_CAPTURE_ID.fetch_add(1, Ordering::Relaxed)
        );
        let published = state.start_with(capture_id.clone(), || {
            let session = echo::rec::RecordingSession::acquire()?;
            let capture = AudioCapture::open_default().map_err(|error| error.to_string())?;
            let cancel = capture.cancel.clone();
            let recording = std::thread::Builder::new()
                .name("echo-dictionary-training".to_string())
                .spawn(move || {
                    let audio = std::thread::scope(|scope| {
                        let stop = capture.cancel.clone();
                        let session_ref = &session;
                        scope.spawn(move || {
                            while !stop.is_cancelled() && !session_ref.stop_requested() {
                                std::thread::sleep(Duration::from_millis(20));
                            }
                            stop.cancel();
                        });
                        let audio = capture.record(MAX_SAMPLE_DURATION, None);
                        capture.cancel.cancel();
                        audio
                    })?;
                    Ok(TrainingCapture { audio, session })
                })
                .map_err(|error| error.to_string())?;
            Ok(ActiveCapture {
                cancel,
                recording: Some(recording),
            })
        })?;
        if published {
            Ok(capture_id)
        } else {
            Err("This voice training capture is no longer active.".to_string())
        }
    })
    .await?
}

#[tauri::command]
pub(crate) async fn finish_dictionary_training_sample(
    captures: State<'_, DictionaryTrainingCaptures>,
    capture_id: String,
) -> Result<DictionaryTrainingSample, String> {
    let active = captures
        .state
        .take_active(&capture_id)?
        .ok_or_else(|| "This voice training capture is no longer active.".to_string())?;
    active.cancel.cancel();
    crate::blocking::run_blocking("voice training transcription", move || {
        let captured = active.finish()?;
        let config = echo::settings::runtime_config()?;
        let prepared = echo::transcribe::prepare_with_config(RunOverrides::default(), &config)
            .map_err(|error| error.to_string())?;
        captured.session.clear_stop_request();
        let transcript = prepared
            .transcribe_bounded(
                &captured.audio.pcm,
                TranscriptionPurpose::DictionaryTraining,
                Instant::now() + Duration::from_secs(15 * 60),
                &|| captured.session.stop_requested(),
            )
            .map_err(|error| error.to_string())?;
        Ok(DictionaryTrainingSample {
            transcript: transcript.raw,
            engine: transcript.engine.to_string(),
        })
    })
    .await?
}

#[tauri::command]
pub(crate) async fn cancel_dictionary_training_sample(
    captures: State<'_, DictionaryTrainingCaptures>,
    capture_id: String,
) -> Result<bool, String> {
    let cancellation = captures.state.cancel(&capture_id)?;
    if !cancellation.accepted() {
        return Ok(false);
    }
    if let Some(active) = cancellation.into_active() {
        active.cancel.cancel();
        crate::blocking::run_blocking("cancel voice training capture", move || drop(active))
            .await?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    struct OpenedCapture(mpsc::SyncSender<()>);

    impl Drop for OpenedCapture {
        fn drop(&mut self) {
            self.0.send(()).expect("opened-capture drop observer");
        }
    }

    #[test]
    fn poisoned_capture_startup_protocol_returns_an_explicit_error() {
        let state = Arc::new(CaptureStartupState::<()>::default());
        let poison = Arc::clone(&state);
        assert!(std::thread::spawn(move || {
            let _guard = poison.phase.lock().unwrap();
            panic!("poison capture startup protocol");
        })
        .join()
        .is_err());

        let error = state
            .start_with("capture-1".to_string(), || Ok(()))
            .unwrap_err();
        assert_eq!(error, "Voice training capture is unavailable.");
    }

    #[test]
    fn runtime_responsiveness_training_start_does_not_block_finish_or_cancel_state() {
        let (dropped_send, dropped_receive) = mpsc::sync_channel(1);
        let state = Arc::new(CaptureStartupState::<OpenedCapture>::default());
        let startup_state = Arc::clone(&state);
        let (device_started_send, device_started_receive) = mpsc::sync_channel(0);
        let (release_device_send, release_device_receive) = mpsc::sync_channel(0);
        let startup = std::thread::spawn(move || {
            startup_state.start_with("capture-1".to_string(), || {
                device_started_send.send(()).expect("device-start observer");
                release_device_receive.recv().expect("device-start release");
                Ok(OpenedCapture(dropped_send))
            })
        });
        device_started_receive
            .recv()
            .expect("startup reached injected device work");

        let finish_state = Arc::clone(&state);
        let (finish_send, finish_receive) = mpsc::sync_channel(0);
        let finish_access = std::thread::spawn(move || {
            finish_send
                .send(
                    finish_state
                        .take_active("capture-1")
                        .expect("finish-state lock")
                        .is_none(),
                )
                .expect("finish-state observer");
        });
        assert!(
            finish_receive.recv().expect("finish-state access result"),
            "finish state access returns while startup device work is still blocked"
        );
        finish_access.join().expect("finish-state thread");

        assert!(
            state
                .cancel("capture-1")
                .expect("cancel-state lock")
                .accepted(),
            "cancel state access returns while startup device work is still blocked"
        );
        release_device_send
            .send(())
            .expect("release injected device work");
        assert!(
            !startup
                .join()
                .expect("startup thread")
                .expect("injected startup result"),
            "a capture cancelled during startup must not be published"
        );
        dropped_receive
            .recv()
            .expect("cancelled startup drops opened resources");
        assert!(state
            .take_active("capture-1")
            .expect("final capture-state lock")
            .is_none());
    }
    #[test]
    fn active_training_rejects_duplicate_start_and_foreign_ids() {
        let state = CaptureStartupState::default();
        assert!(state.start_with("capture-a".into(), || Ok(42)).unwrap());
        assert!(state
            .start_with("capture-b".into(), || panic!("duplicate opened a device"))
            .is_err());
        assert!(state.take_active("capture-b").unwrap().is_none());
        assert!(!state.cancel("capture-b").unwrap().accepted());
        assert_eq!(state.take_active("capture-a").unwrap(), Some(42));
        assert!(!state.cancel("capture-a").unwrap().accepted());
    }

    #[test]
    fn cancelling_active_training_releases_its_capture_once() {
        let (dropped_send, dropped_receive) = mpsc::sync_channel(1);
        let state = CaptureStartupState::default();
        assert!(state
            .start_with("capture-a".into(), || Ok(OpenedCapture(dropped_send)))
            .unwrap());
        let cancelled = state.cancel("capture-a").unwrap();
        assert!(cancelled.accepted());
        assert!(!state.cancel("capture-a").unwrap().accepted());
        assert!(state.take_active("capture-a").unwrap().is_none());
        assert!(dropped_receive.try_recv().is_err());
        drop(cancelled);
        dropped_receive.recv().unwrap();
        assert!(dropped_receive.try_recv().is_err());
    }
}
