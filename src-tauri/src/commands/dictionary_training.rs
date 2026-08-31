use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::Duration;

use echo::audio::{AudioCapture, CancellationToken, CaptureResult};
use echo::transcribe::{RunOverrides, TranscriptionPurpose};
use echo_desktop::ipc::DictionaryTrainingSample;
use tauri::State;

const MAX_SAMPLE_DURATION: Duration = Duration::from_secs(30);
static NEXT_CAPTURE_ID: AtomicU64 = AtomicU64::new(1);

struct ActiveCapture {
    id: String,
    cancel: CancellationToken,
    recording: JoinHandle<Result<TrainingCapture, echo::audio::AudioError>>,
}

struct TrainingCapture {
    audio: CaptureResult,
    _session: echo::rec::RecordingSession,
}

#[derive(Default)]
pub(crate) struct DictionaryTrainingCaptures {
    active: Mutex<Option<ActiveCapture>>,
}

impl DictionaryTrainingCaptures {
    fn take(&self, capture_id: &str) -> Result<Option<ActiveCapture>, String> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| "Voice training capture is unavailable.".to_string())?;
        if active
            .as_ref()
            .is_some_and(|capture| capture.id == capture_id)
        {
            Ok(active.take())
        } else {
            Ok(None)
        }
    }
}

impl Drop for DictionaryTrainingCaptures {
    fn drop(&mut self) {
        let Ok(active) = self.active.get_mut() else {
            return;
        };
        if let Some(active) = active.take() {
            active.cancel.cancel();
            let _ = active.recording.join();
        }
    }
}

#[tauri::command]
pub(crate) fn start_dictionary_training_sample(
    captures: State<'_, DictionaryTrainingCaptures>,
) -> Result<String, String> {
    let mut active = captures
        .active
        .lock()
        .map_err(|_| "Voice training capture is unavailable.".to_string())?;
    if active.is_some() {
        return Err("A voice training sample is already recording.".to_string());
    }

    let session = echo::rec::RecordingSession::acquire()?;
    let capture = AudioCapture::open_default().map_err(|error| error.to_string())?;
    let cancel = capture.cancel.clone();
    let capture_id = format!(
        "{}-{}",
        std::process::id(),
        NEXT_CAPTURE_ID.fetch_add(1, Ordering::Relaxed)
    );
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
            Ok(TrainingCapture {
                audio,
                _session: session,
            })
        })
        .map_err(|error| error.to_string())?;
    *active = Some(ActiveCapture {
        id: capture_id.clone(),
        cancel,
        recording,
    });
    Ok(capture_id)
}

#[tauri::command]
pub(crate) async fn finish_dictionary_training_sample(
    captures: State<'_, DictionaryTrainingCaptures>,
    capture_id: String,
) -> Result<DictionaryTrainingSample, String> {
    let active = captures
        .take(&capture_id)?
        .ok_or_else(|| "This voice training capture is no longer active.".to_string())?;
    active.cancel.cancel();
    crate::blocking::run_blocking("voice training transcription", move || {
        let captured = active
            .recording
            .join()
            .map_err(|_| "Voice training capture stopped unexpectedly.".to_string())?
            .map_err(|error| error.to_string())?;
        let prepared = echo::transcribe::prepare_with_config(
            RunOverrides::default(),
            &echo::settings::file_config(),
        )
        .map_err(|error| error.to_string())?;
        let transcript = prepared
            .transcribe(
                &captured.audio.pcm,
                TranscriptionPurpose::DictionaryTraining,
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
    let Some(active) = captures.take(&capture_id)? else {
        return Ok(false);
    };
    active.cancel.cancel();
    crate::blocking::run_blocking("cancel voice training capture", move || {
        let _ = active.recording.join();
    })
    .await?;
    Ok(true)
}
