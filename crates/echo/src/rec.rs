use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use echo_core::{
    Dictionary, FailReason, History, HistoryRow, InjectReport, Injector, Session, SessionState,
};

use crate::audio::{self, AudioCapture, CancellationToken};
use crate::hotkey::HotkeyEvent;
use crate::inject::LinuxInjector;
use crate::status;

/// Set while this process holds a recording session, so the GUI can tell its
/// own record button's sessions (live meter) from a compositor shortcut's
/// (the meter lives in that process).
static RECORDING_IN_PROCESS: AtomicBool = AtomicBool::new(false);

#[must_use]
pub fn recording_in_process() -> bool {
    RECORDING_IN_PROCESS.load(Ordering::Relaxed)
}

struct InProcessSession;

impl InProcessSession {
    fn start() -> Self {
        RECORDING_IN_PROCESS.store(true, Ordering::Relaxed);
        Self
    }
}

impl Drop for InProcessSession {
    fn drop(&mut self) {
        RECORDING_IN_PROCESS.store(false, Ordering::Relaxed);
        audio::process_meter().publish(0.0);
    }
}

enum StopWhen {
    Timer,
    ToggleFile(ToggleSession),
}

pub fn run_rec_once() -> i32 {
    run_record(StopWhen::Timer)
}

pub fn run_rec_toggle() -> i32 {
    match ToggleSession::start_or_stop() {
        Ok(action) => {
            if let Err(err) = status::mark_shortcut_activation("toggle-command") {
                eprintln!("toggle: cannot record shortcut provenance: {err}");
            }
            match action {
                ToggleAction::Start(session) => run_record(StopWhen::ToggleFile(session)),
                ToggleAction::Stop => 0,
            }
        }
        Err(err) => {
            eprintln!("toggle: {err}");
            1
        }
    }
}

/// Toggle an in-process recording after synchronously acquiring or stopping
/// the cross-process session. Recording work continues on a background thread.
pub fn toggle_managed_recording() -> Result<(), String> {
    match ToggleSession::start_or_stop()? {
        ToggleAction::Start(session) => std::thread::Builder::new()
            .name("echo-record-toggle".to_string())
            .spawn(move || {
                let _ = run_record(StopWhen::ToggleFile(session));
            })
            .map(|_| ())
            .map_err(|err| err.to_string()),
        ToggleAction::Stop => Ok(()),
    }
}

fn run_record(mut stop: StopWhen) -> i32 {
    let mut session = Session::new();
    log_state(&session);
    let _ = status::write_status(session.state(), None, None);
    apply_edge(&mut session, HotkeyEvent::Down);
    let _ = status::write_status(session.state(), None, None);
    // The HUD lives until after injection: the longest wait in the session
    // (transcription) gets an indicator, and the outcome gets a state.
    let _in_process = InProcessSession::start();
    let meter = audio::process_meter();
    let hud = crate::ui::hud::RecordingHud::start(meter.clone());
    let capture = match capture_pcm(&mut stop, &meter) {
        Ok(capture) => capture,
        Err(reason) => {
            hud.set_state(crate::ui::hud::HudState::Failed);
            let _ = session.fail(reason);
            log_state(&session);
            let _ = status::write_status(session.state(), None, None);
            crate::notify::notify_session_failure(reason, None);
            return 1;
        }
    };
    hud.set_state(crate::ui::hud::HudState::Transcribing);
    apply_edge(&mut session, HotkeyEvent::Up);
    let _ = status::write_status(session.state(), None, None);

    let dict = Dictionary::load().unwrap_or_else(|_| Dictionary::empty());
    let prepared = match crate::transcribe::prepare_with_config(
        crate::transcribe::RunOverrides::default(),
        &crate::settings::file_config(),
    ) {
        Ok(prepared) => prepared,
        Err(err) => {
            let reason = match &err {
                crate::transcribe::PrepareError::EngineMissing(_) => FailReason::EngineMissing,
                crate::transcribe::PrepareError::InvalidRequest(_)
                | crate::transcribe::PrepareError::Configuration(_) => FailReason::EngineError,
            };
            let _ = session.fail(reason);
            log_state(&session);
            let detail = err.to_string();
            let _ = status::write_status(session.state(), None, Some(&detail));
            eprintln!("{detail}");
            crate::notify::notify_session_failure(reason, Some(&detail));
            return 1;
        }
    };
    let transcript = match prepared.transcribe(
        &capture.pcm,
        &dict,
        crate::transcribe::CleanupPolicy::DictionaryFallback,
    ) {
        Ok(transcript) => transcript,
        Err(err) => {
            hud.set_state(crate::ui::hud::HudState::Failed);
            let (reason, detail) = match &err {
                crate::transcribe::TranscriptionError::Engine(echo_core::EngineError::Missing) => {
                    (FailReason::EngineMissing, None)
                }
                _ => (FailReason::EngineError, None),
            };
            let message = err.to_string();
            let detail = detail.or(Some(message.as_str()));
            let _ = session.fail(reason);
            log_state(&session);
            let _ = status::write_status(session.state(), None, detail);
            crate::notify::notify_session_failure(reason, detail);
            return 1;
        }
    };

    if session.begin_cleaning().is_ok() {
        log_state(&session);
    }
    if session.begin_injecting().is_ok() {
        log_state(&session);
    }

    let inject = if skip_inject() {
        InjectReport::ClipboardOnly
    } else {
        let injector = LinuxInjector::new();
        match injector.focus() {
            Ok(target) => injector.inject(&transcript.text, &target),
            Err(reason) => InjectReport::Failed { reason },
        }
    };
    let failed = inject.failed();
    if failed {
        let reason = match &inject {
            InjectReport::Failed { reason } => *reason,
            _ => FailReason::InjectUnconfirmed,
        };
        hud.set_state(crate::ui::hud::HudState::Failed);
        let _ = session.fail(reason);
        log_state(&session);
        crate::notify::notify_session_failure(reason, None);
    } else if session.complete_inject().is_ok() {
        hud.set_state(crate::ui::hud::HudState::Done);
        log_state(&session);
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let started_at = now.as_secs();
    if let Ok(mut history) = History::load() {
        let _ = history.append(HistoryRow {
            // Nanoseconds plus pid keep ids unique across processes and after
            // the store trims to its row cap.
            id: format!("{started_at}-{}-{}", now.subsec_nanos(), std::process::id()),
            text: transcript.text.clone(),
            raw: transcript.raw.clone(),
            engine: transcript.engine.clone(),
            started_at,
            infer_ms: transcript.infer_ms,
            inject,
            detail: transcript.detail.clone(),
        });
    }
    if failed {
        // Leave the Failed state visible; the next session overwrites it.
        let _ = status::write_status(session.state(), None, None);
        return 1;
    }
    let _ = status::write_status(session.state(), Some(&transcript.text), None);
    0
}

fn skip_inject() -> bool {
    matches!(
        std::env::var("ECHO_SKIP_INJECT").ok().as_deref(),
        Some("1") | Some("true")
    )
}

fn apply_edge(session: &mut Session, event: HotkeyEvent) {
    let result = match event {
        HotkeyEvent::Down => session.start_recording(),
        HotkeyEvent::Up => session.finish_recording(),
    };
    match result {
        Ok(()) => log_state(session),
        Err(err) => eprintln!("session error: {err}"),
    }
}

fn capture_pcm(
    stop: &mut StopWhen,
    meter: &audio::LevelMeter,
) -> Result<audio::CaptureResult, FailReason> {
    capture_from(fixture_path(), stop, meter)
}

fn capture_from(
    fixture: Option<PathBuf>,
    stop: &mut StopWhen,
    meter: &audio::LevelMeter,
) -> Result<audio::CaptureResult, FailReason> {
    if let Some(path) = fixture {
        let capture = audio::load_wav(&path).map_err(|_| FailReason::EngineError)?;
        // Publish the fixture's loudness at real-time cadence so the HUD's
        // bars are truthful in demos and CI screenshots.
        let playback_cancel = CancellationToken::new();
        let player = audio::play_fixture_meter(&capture.pcm, meter.clone(), playback_cancel);
        let _ = player.join();
        return Ok(capture);
    }
    let capture = AudioCapture::open_default().map_err(|err| match err {
        audio::AudioError::NoDevice => FailReason::NoInputDevice,
        _ => FailReason::CaptureFailed,
    })?;
    let result = match stop {
        StopWhen::Timer => capture.record(recording_duration(), Some(meter)),
        StopWhen::ToggleFile(toggle) => {
            let cancel = capture.cancel.clone();
            let stop_path = &toggle.stop_path;
            std::thread::scope(|scope| {
                scope.spawn(|| {
                    while !cancel.is_cancelled() && !stop_path.exists() {
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    cancel.cancel();
                });
                let result = capture.record(Duration::from_secs(MAX_RECORD_SECONDS), Some(meter));
                capture.cancel.cancel();
                result
            })
        }
    };
    result.map_err(|_| FailReason::CaptureFailed)
}

enum ToggleAction {
    Start(ToggleSession),
    Stop,
}

struct ToggleSession {
    lock_path: PathBuf,
    stop_path: PathBuf,
}

impl ToggleSession {
    fn start_or_stop() -> Result<ToggleAction, String> {
        Self::start_or_stop_in(&echo_core::data_dir())
    }

    fn start_or_stop_in(dir: &Path) -> Result<ToggleAction, String> {
        if let Some(session) = Self::try_start_in(dir)? {
            return Ok(ToggleAction::Start(session));
        }
        fs::write(dir.join("recording.stop"), b"stop\n").map_err(|err| err.to_string())?;
        Ok(ToggleAction::Stop)
    }

    fn try_start_in(dir: &Path) -> Result<Option<Self>, String> {
        fs::create_dir_all(dir).map_err(|err| err.to_string())?;
        let lock_path = dir.join("recording.lock");
        let stop_path = dir.join("recording.stop");
        for _ in 0..2 {
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock_path)
            {
                Ok(mut lock) => {
                    let _ = fs::remove_file(&stop_path);
                    writeln!(lock, "{}", std::process::id()).map_err(|err| err.to_string())?;
                    return Ok(Some(Self {
                        lock_path,
                        stop_path,
                    }));
                }
                Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                    if lock_owner_is_alive(&lock_path) {
                        return Ok(None);
                    }
                    let _ = fs::remove_file(&lock_path);
                    let _ = fs::remove_file(&stop_path);
                }
                Err(err) => return Err(err.to_string()),
            }
        }
        Err("could not acquire recording lock".to_string())
    }
}

impl Drop for ToggleSession {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.stop_path);
        let _ = fs::remove_file(&self.lock_path);
    }
}

fn lock_owner_is_alive(path: &Path) -> bool {
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| raw.trim().parse::<u32>().ok())
        .map(|pid| PathBuf::from("/proc").join(pid.to_string()).exists())
        .unwrap_or(false)
}

/// True while any process holds an active recording session.
#[must_use]
pub fn session_active() -> bool {
    lock_owner_is_alive(&echo_core::data_dir().join("recording.lock"))
}

/// Ceiling for any recording, in seconds.
pub const MAX_RECORD_SECONDS: u64 = 60;

fn record_seconds(env: Option<u64>, file: Option<u32>) -> u64 {
    echo_core::resolve(env, file.map(u64::from), 3).clamp(1, MAX_RECORD_SECONDS)
}

fn recording_duration() -> Duration {
    let env = std::env::var("ECHO_RECORD_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok());
    Duration::from_secs(record_seconds(
        env,
        crate::settings::file_config().record_seconds,
    ))
}

fn fixture_path() -> Option<PathBuf> {
    std::env::var_os("ECHO_AUDIO_FIXTURE").map(PathBuf::from)
}

fn log_state(session: &Session) {
    let name = match session.state() {
        SessionState::Idle => "Idle",
        SessionState::Recording { .. } => "Recording",
        SessionState::Transcribing => "Transcribing",
        SessionState::Cleaning => "Cleaning",
        SessionState::Injecting => "Injecting",
        SessionState::Failed { reason } => {
            println!("session Failed {}", reason.as_str());
            return;
        }
    };
    println!("session {name}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_seconds_prefers_env_then_file_and_clamps() {
        assert_eq!(record_seconds(Some(8), Some(12)), 8);
        assert_eq!(record_seconds(None, Some(12)), 12);
        assert_eq!(record_seconds(None, None), 3);
        assert_eq!(record_seconds(Some(0), None), 1);
        assert_eq!(record_seconds(Some(90), None), MAX_RECORD_SECONDS);
    }

    #[test]
    fn fixture_returns_wav_without_opening_host() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude_code.wav");
        let mut stop = StopWhen::Timer;
        let capture =
            capture_from(Some(path), &mut stop, &audio::LevelMeter::new()).expect("fixture wav");
        assert!(capture.pcm.duration_ms() >= 300);
        assert!(capture.peak_rms > 0.05);
    }

    #[test]
    fn fixture_publishes_its_loudness_to_the_meter() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude_code.wav");
        let mut stop = StopWhen::Timer;
        let meter = audio::LevelMeter::new();
        let probe = meter.clone();
        let peak = std::thread::spawn(move || {
            let mut peak = 0.0f32;
            for _ in 0..400 {
                peak = peak.max(probe.level());
                std::thread::sleep(Duration::from_millis(5));
            }
            peak
        });
        let _ = capture_from(Some(path), &mut stop, &meter).expect("fixture wav");
        let peak = peak.join().expect("probe thread");
        assert!(peak > 0.01, "fixture playback moved the meter: {peak}");
    }

    #[test]
    fn toggle_starts_stops_and_can_restart() {
        let dir = std::env::temp_dir().join(format!("echo-toggle-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);

        let first = match ToggleSession::start_or_stop_in(&dir).unwrap() {
            ToggleAction::Start(session) => session,
            ToggleAction::Stop => panic!("first toggle should start"),
        };
        assert!(first.lock_path.is_file());
        assert!(matches!(
            ToggleSession::start_or_stop_in(&dir).unwrap(),
            ToggleAction::Stop
        ));
        assert!(first.stop_path.is_file());

        drop(first);
        assert!(!dir.join("recording.lock").exists());
        assert!(!dir.join("recording.stop").exists());
        assert!(matches!(
            ToggleSession::start_or_stop_in(&dir).unwrap(),
            ToggleAction::Start(_)
        ));
    }
}
