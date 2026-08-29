use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use echo_core::{
    Dictionary, FailReason, History, HistoryRow, InjectReport, Injector, Pcm16kMono,
    RecordingLimit, ResolvedRecordingLimit, Session, SessionState, SAMPLE_RATE_HZ,
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
            let recording_token = action.recording_token().map(str::to_string);
            if let Err(err) =
                status::mark_shortcut_activation("toggle-command", recording_token.as_deref())
            {
                eprintln!("toggle: cannot record shortcut provenance: {err}");
            }
            match action {
                ToggleAction::Start(session) => run_record(StopWhen::ToggleFile(session)),
                ToggleAction::Stop(_) => 0,
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
pub fn toggle_managed_recording() -> Result<Option<String>, String> {
    match ToggleSession::start_or_stop()? {
        ToggleAction::Start(session) => {
            let recording_token = session.token.clone();
            std::thread::Builder::new()
                .name("echo-record-toggle".to_string())
                .spawn(move || {
                    let _ = run_record(StopWhen::ToggleFile(session));
                })
                .map(|_| Some(recording_token))
                .map_err(|err| err.to_string())
        }
        ToggleAction::Stop(owner) => Ok(owner.token),
    }
}

pub fn stop_shortcut_recording(activation: &str) -> Result<bool, String> {
    let current = status::shortcut_activation();
    if current.as_deref().map(str::trim) != Some(activation.trim()) {
        return Ok(false);
    }
    let Some(recording_token) = status::shortcut_recording_token(activation) else {
        return Ok(false);
    };
    ToggleSession::request_stop_for_token_in(&echo_core::data_dir(), recording_token)
}

fn run_record(stop: StopWhen) -> i32 {
    let limit = recording_limit_from_process().limit;
    run_record_with_limit(stop, limit)
}

fn run_record_with_limit(mut stop: StopWhen, limit: RecordingLimit) -> i32 {
    let mut session = Session::new();
    log_state(&session);
    let _ = status::write_status(session.state(), None, None);
    apply_edge(&mut session, HotkeyEvent::Down);
    let _ = status::write_recording(limit);
    // The HUD lives until after injection: the longest wait in the session
    // (transcription) gets an indicator, and the outcome gets a state.
    let _in_process = InProcessSession::start();
    let meter = audio::process_meter();
    let hud = crate::ui::hud::RecordingHud::start(meter.clone());
    let capture = match capture_pcm(&mut stop, limit, &meter) {
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
    limit: RecordingLimit,
    meter: &audio::LevelMeter,
) -> Result<audio::CaptureResult, FailReason> {
    capture_from(fixture_path(), stop, limit, meter)
}

fn capture_from(
    fixture: Option<PathBuf>,
    stop: &mut StopWhen,
    limit: RecordingLimit,
    meter: &audio::LevelMeter,
) -> Result<audio::CaptureResult, FailReason> {
    if let Some(path) = fixture {
        let capture = audio::load_wav(&path).map_err(|_| FailReason::EngineError)?;
        return play_fixture_capture(capture, stop, limit, meter);
    }
    let capture = AudioCapture::open_default().map_err(|err| match err {
        audio::AudioError::NoDevice => FailReason::NoInputDevice,
        _ => FailReason::CaptureFailed,
    })?;
    record_device(&capture, stop, limit, meter).map_err(|_| FailReason::CaptureFailed)
}

fn play_fixture_capture(
    capture: audio::CaptureResult,
    stop: &StopWhen,
    limit: RecordingLimit,
    meter: &audio::LevelMeter,
) -> Result<audio::CaptureResult, FailReason> {
    let max_samples = (limit.seconds() as usize)
        .saturating_mul(SAMPLE_RATE_HZ as usize)
        .min(capture.pcm.len());
    let pcm = Pcm16kMono::from_samples(capture.pcm.samples()[..max_samples].to_vec());
    let cancel = CancellationToken::new();

    let played = std::thread::scope(|scope| {
        spawn_toggle_stop_watcher(scope, stop, cancel.clone());
        let player = audio::play_fixture_meter(&pcm, meter.clone(), cancel.clone());
        let played = player.join().unwrap_or(0);
        cancel.cancel();
        played
    });
    let played = played.min(pcm.len());
    Ok(audio::CaptureResult::from_pcm(Pcm16kMono::from_samples(
        pcm.samples()[..played].to_vec(),
    )))
}

fn record_device(
    capture: &AudioCapture,
    stop: &mut StopWhen,
    limit: RecordingLimit,
    meter: &audio::LevelMeter,
) -> Result<audio::CaptureResult, audio::AudioError> {
    std::thread::scope(|scope| {
        spawn_toggle_stop_watcher(scope, stop, capture.cancel.clone());
        let result = capture.record(limit.duration(), Some(meter));
        capture.cancel.cancel();
        result
    })
}

fn spawn_toggle_stop_watcher<'scope>(
    scope: &'scope std::thread::Scope<'scope, '_>,
    stop: &'scope StopWhen,
    cancel: CancellationToken,
) {
    if let StopWhen::ToggleFile(toggle) = stop {
        scope.spawn(move || {
            while !cancel.is_cancelled() && !toggle.stop_requested() {
                std::thread::sleep(Duration::from_millis(20));
            }
            cancel.cancel();
        });
    }
}

enum ToggleAction {
    Start(ToggleSession),
    Stop(LockOwner),
}

impl ToggleAction {
    fn recording_token(&self) -> Option<&str> {
        match self {
            Self::Start(session) => Some(&session.token),
            Self::Stop(owner) => owner.token.as_deref(),
        }
    }
}

enum LockAcquisition {
    Started(ToggleSession),
    Busy(LockOwner),
}

struct ToggleSession {
    lock_path: PathBuf,
    stop_path: PathBuf,
    token: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LockOwner {
    pid: u32,
    token: Option<String>,
}

impl ToggleSession {
    fn start_or_stop() -> Result<ToggleAction, String> {
        Self::start_or_stop_in(&echo_core::data_dir())
    }

    fn start_or_stop_in(dir: &Path) -> Result<ToggleAction, String> {
        match Self::acquire_in(dir)? {
            LockAcquisition::Started(session) => Ok(ToggleAction::Start(session)),
            LockAcquisition::Busy(owner) => {
                write_stop_request(&dir.join("recording.stop"), &owner)?;
                Ok(ToggleAction::Stop(owner))
            }
        }
    }

    #[cfg(test)]
    fn request_stop_if_active_in(dir: &Path) -> Result<bool, String> {
        let lock_path = dir.join("recording.lock");
        let stop_path = dir.join("recording.stop");
        let Some(owner) = live_lock_owner(&lock_path) else {
            let _ = fs::remove_file(lock_path);
            let _ = fs::remove_file(stop_path);
            return Ok(false);
        };
        write_stop_request(&stop_path, &owner)?;
        Ok(true)
    }

    fn request_stop_for_token_in(dir: &Path, token: &str) -> Result<bool, String> {
        let lock_path = dir.join("recording.lock");
        let Some(owner) = live_lock_owner(&lock_path) else {
            return Ok(false);
        };
        if owner.token.as_deref() != Some(token) {
            return Ok(false);
        }
        write_stop_request(&dir.join("recording.stop"), &owner)?;
        Ok(true)
    }

    #[cfg(test)]
    fn try_start_in(dir: &Path) -> Result<Option<Self>, String> {
        match Self::acquire_in(dir)? {
            LockAcquisition::Started(session) => Ok(Some(session)),
            LockAcquisition::Busy(_) => Ok(None),
        }
    }

    fn acquire_in(dir: &Path) -> Result<LockAcquisition, String> {
        fs::create_dir_all(dir).map_err(|err| err.to_string())?;
        let lock_path = dir.join("recording.lock");
        let stop_path = dir.join("recording.stop");
        for _ in 0..2 {
            let token = new_session_token();
            let candidate = dir.join(format!(".recording.lock.{token}"));
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
            {
                Ok(mut lock) => {
                    writeln!(lock, "{}\n{token}", std::process::id())
                        .map_err(|err| err.to_string())?;
                    drop(lock);
                    match fs::hard_link(&candidate, &lock_path) {
                        Ok(()) => {}
                        Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                            let _ = fs::remove_file(&candidate);
                            if let Some(owner) = live_lock_owner(&lock_path) {
                                return Ok(LockAcquisition::Busy(owner));
                            }
                            let _ = fs::remove_file(&lock_path);
                            let _ = fs::remove_file(&stop_path);
                            continue;
                        }
                        Err(err) => {
                            let _ = fs::remove_file(&candidate);
                            return Err(err.to_string());
                        }
                    }
                    let _ = fs::remove_file(&candidate);
                    return Ok(LockAcquisition::Started(Self {
                        lock_path,
                        stop_path,
                        token,
                    }));
                }
                Err(err) => return Err(err.to_string()),
            }
        }
        Err("could not acquire recording lock".to_string())
    }

    fn stop_requested(&self) -> bool {
        fs::read_to_string(&self.stop_path)
            .ok()
            .is_some_and(|token| token.trim() == self.token)
    }
}

impl Drop for ToggleSession {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.stop_path);
        let _ = fs::remove_file(&self.lock_path);
    }
}

fn new_session_token() -> String {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!(
        "{}-{}-{}-{}",
        std::process::id(),
        now.as_secs(),
        now.subsec_nanos(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}

fn lock_owner(path: &Path) -> Option<LockOwner> {
    let raw = fs::read_to_string(path).ok()?;
    let mut lines = raw.lines();
    let pid = lines.next()?.trim().parse().ok()?;
    let token = lines
        .next()
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string);
    Some(LockOwner { pid, token })
}

fn live_lock_owner(path: &Path) -> Option<LockOwner> {
    lock_owner(path).filter(|owner| process_is_alive(owner.pid))
}

fn lock_owner_is_alive(path: &Path) -> bool {
    live_lock_owner(path).is_some()
}

fn write_stop_request(path: &Path, owner: &LockOwner) -> Result<(), String> {
    let contents = owner.token.as_deref().unwrap_or("stop");
    echo_core::write_atomic(path, format!("{contents}\n").as_bytes())
}

#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    let Some(pid) = rustix::process::Pid::from_raw(pid as rustix::process::RawPid) else {
        return false;
    };
    matches!(
        rustix::process::test_kill_process(pid),
        Ok(()) | Err(rustix::io::Errno::PERM)
    )
}

#[cfg(not(unix))]
fn process_is_alive(pid: u32) -> bool {
    PathBuf::from("/proc").join(pid.to_string()).exists()
}

/// True while any process holds an active recording session.
#[must_use]
pub fn session_active() -> bool {
    session_active_at(&echo_core::data_dir().join("recording.lock"))
}

pub(crate) fn session_active_at(path: &Path) -> bool {
    lock_owner_is_alive(path)
}

#[must_use]
pub fn recording_limit_from_process() -> ResolvedRecordingLimit {
    let environment = std::env::var("ECHO_RECORD_SECONDS").ok();
    echo_core::resolve_recording_limit(
        environment.as_deref(),
        crate::settings::file_config().record_seconds,
    )
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
    fn stop_only_never_starts_a_recording() {
        let dir = std::env::temp_dir().join(format!("echo-stop-only-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);

        assert!(!ToggleSession::request_stop_if_active_in(&dir).unwrap());
        assert!(!dir.join("recording.lock").exists());
        assert!(!dir.join("recording.stop").exists());

        let session = ToggleSession::try_start_in(&dir).unwrap().unwrap();
        assert!(ToggleSession::request_stop_if_active_in(&dir).unwrap());
        assert!(ToggleSession::request_stop_if_active_in(&dir).unwrap());
        assert!(dir.join("recording.lock").exists());
        assert!(dir.join("recording.stop").exists());

        drop(session);
        assert!(!ToggleSession::request_stop_if_active_in(&dir).unwrap());
        assert!(!dir.join("recording.lock").exists());
        assert!(!dir.join("recording.stop").exists());
    }

    #[test]
    fn token_scoped_stop_ignores_an_unrelated_session() {
        let dir = std::env::temp_dir().join(format!("echo-scoped-stop-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let session = ToggleSession::try_start_in(&dir).unwrap().unwrap();

        assert!(!ToggleSession::request_stop_for_token_in(&dir, "another-session").unwrap());
        assert!(!session.stop_path.exists());
        assert!(ToggleSession::request_stop_for_token_in(&dir, &session.token).unwrap());
        assert!(session.stop_requested());
    }

    #[test]
    fn stale_stop_request_cannot_cancel_a_replacement_session() {
        let dir = std::env::temp_dir().join(format!("echo-stop-token-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);

        let first = ToggleSession::try_start_in(&dir).unwrap().unwrap();
        let first_owner = lock_owner(&first.lock_path).unwrap();
        drop(first);
        let second = ToggleSession::try_start_in(&dir).unwrap().unwrap();

        write_stop_request(&second.stop_path, &first_owner).unwrap();
        assert!(!second.stop_requested());
        assert_ne!(first_owner.token.as_deref(), Some(second.token.as_str()));
    }

    #[test]
    fn fixture_returns_wav_without_opening_host() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude_code.wav");
        let mut stop = StopWhen::Timer;
        let capture = capture_from(
            Some(path),
            &mut stop,
            RecordingLimit::DEFAULT,
            &audio::LevelMeter::new(),
        )
        .expect("fixture wav");
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
        let _ = capture_from(Some(path), &mut stop, RecordingLimit::DEFAULT, &meter)
            .expect("fixture wav");
        let peak = peak.join().expect("probe thread");
        assert!(peak > 0.01, "fixture playback moved the meter: {peak}");
    }

    #[test]
    fn fixture_obeys_the_snapped_limit() {
        let pcm = Pcm16kMono::from_samples(vec![i16::MAX / 4; SAMPLE_RATE_HZ as usize * 2]);
        let capture = audio::CaptureResult::from_pcm(pcm);
        let result = play_fixture_capture(
            capture,
            &StopWhen::Timer,
            RecordingLimit::MIN,
            &audio::LevelMeter::new(),
        )
        .unwrap();

        assert_eq!(result.pcm.len(), SAMPLE_RATE_HZ as usize);
        assert_eq!(result.duration, Duration::from_secs(1));
    }

    #[test]
    fn fixture_obeys_a_token_scoped_toggle_stop() {
        let dir = std::env::temp_dir().join(format!("echo-fixture-stop-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let session = ToggleSession::try_start_in(&dir).unwrap().unwrap();
        let pcm = Pcm16kMono::from_samples(vec![i16::MAX / 4; SAMPLE_RATE_HZ as usize * 2]);
        let capture = audio::CaptureResult::from_pcm(pcm);
        let stopper = std::thread::spawn({
            let dir = dir.clone();
            move || {
                std::thread::sleep(Duration::from_millis(80));
                ToggleSession::request_stop_if_active_in(&dir).unwrap()
            }
        });

        let result = play_fixture_capture(
            capture,
            &StopWhen::ToggleFile(session),
            RecordingLimit::MAX,
            &audio::LevelMeter::new(),
        )
        .unwrap();

        assert!(stopper.join().unwrap());
        assert!(result.duration < Duration::from_millis(500));
    }

    #[test]
    fn toggle_starts_stops_and_can_restart() {
        let dir = std::env::temp_dir().join(format!("echo-toggle-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);

        let first = match ToggleSession::start_or_stop_in(&dir).unwrap() {
            ToggleAction::Start(session) => session,
            ToggleAction::Stop(_) => panic!("first toggle should start"),
        };
        assert!(first.lock_path.is_file());
        assert!(matches!(
            ToggleSession::start_or_stop_in(&dir).unwrap(),
            ToggleAction::Stop(_)
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
