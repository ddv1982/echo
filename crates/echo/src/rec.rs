use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use echo_core::{
    Dictionary, FailReason, History, HistoryRow, InjectReport, Injector, Pcm16kMono, PrivateDir,
    RecordingLimit, ResolvedRecordingLimit, Session, SessionState, SAMPLE_RATE_HZ,
};
use fs2::FileExt;

use crate::audio::{self, AudioCapture, CancellationToken};
use crate::hotkey::HotkeyEvent;
use crate::inject::LinuxInjector;
use crate::process_identity::{observe as read_process_observation, ProcessObservation};
use crate::status;

/// Set while this process holds a recording session, so the GUI can tell its
/// own record button's sessions (live meter) from a compositor shortcut's
/// (the meter lives in that process).
static RECORDING_IN_PROCESS: AtomicBool = AtomicBool::new(false);
static COMMITTED_TAKEOVER: Mutex<Option<ToggleSession>> = Mutex::new(None);

pub struct TakeoverReservation(Option<ToggleSession>);

impl TakeoverReservation {
    fn commit(mut self) {
        let session = self.0.take().expect("takeover reservation");
        *COMMITTED_TAKEOVER.lock().expect("committed takeover lock") = Some(session);
    }
}

#[derive(Debug)]
pub enum UpgradeTakeover {
    Deferred,
    Spawned,
    SpawnFailed(std::io::Error),
}

/// Reserve the final idle decision, check the cross-process recording lock,
/// and spawn the replacement. A failed spawn reopens local recording; a
/// successful spawn leaves it blocked until this process exits.
pub fn attempt_upgrade_takeover(spawn: impl FnOnce() -> std::io::Result<()>) -> UpgradeTakeover {
    attempt_upgrade_takeover_in(&echo_core::data_dir(), spawn)
}

fn attempt_upgrade_takeover_in(
    dir: &Path,
    spawn: impl FnOnce() -> std::io::Result<()>,
) -> UpgradeTakeover {
    let reservation = match reserve_upgrade_takeover_in(dir) {
        Ok(reservation) => reservation,
        Err(_) => return UpgradeTakeover::Deferred,
    };
    match spawn() {
        Ok(()) => {
            reservation.commit();
            UpgradeTakeover::Spawned
        }
        Err(err) => UpgradeTakeover::SpawnFailed(err),
    }
}

pub fn reserve_upgrade_takeover() -> Result<TakeoverReservation, String> {
    reserve_upgrade_takeover_in(&echo_core::data_dir())
}

fn reserve_upgrade_takeover_in(dir: &Path) -> Result<TakeoverReservation, String> {
    match ToggleSession::acquire_in(dir)? {
        LockAcquisition::Started(session) => Ok(TakeoverReservation(Some(session))),
        LockAcquisition::Busy(_) => Err("recording is active".to_string()),
    }
}

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
    Timer(Option<ToggleSession>),
    ToggleFile(ToggleSession),
}

impl StopWhen {
    fn session(&self) -> Option<&ToggleSession> {
        match self {
            Self::Timer(session) => session.as_ref(),
            Self::ToggleFile(session) => Some(session),
        }
    }

    fn clear_stop_request(&self) {
        if let Some(session) = self.session() {
            session.clear_stop_request();
        }
    }

    fn stop_requested(&self) -> bool {
        self.session().is_some_and(ToggleSession::stop_requested)
    }
}

pub fn run_rec_once() -> i32 {
    match RecordingSession::acquire() {
        Ok(RecordingSession(session)) => run_record(StopWhen::Timer(Some(session))),
        Err(err) => {
            eprintln!("rec: {err}");
            1
        }
    }
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
    let config = match crate::settings::runtime_config() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            let state = SessionState::Failed {
                reason: FailReason::EngineError,
            };
            let _ = status::write_status(state, None, Some(&error), None);
            crate::notify::notify_session_failure(FailReason::EngineError, Some(&error));
            return 1;
        }
    };
    let environment = std::env::var("ECHO_RECORD_SECONDS").ok();
    let limit =
        echo_core::resolve_recording_limit(environment.as_deref(), config.record_seconds).limit;
    run_record_with_limit(stop, limit, &config)
}

fn run_record_with_limit(
    mut stop: StopWhen,
    limit: RecordingLimit,
    config: &echo_core::Config,
) -> i32 {
    let mut session = Session::new();
    log_state(&session);
    let _ = status::write_status(session.state(), None, None, None);
    apply_edge(&mut session, HotkeyEvent::Down);
    let _ = status::write_recording(limit);
    // The HUD lives until after injection: the longest wait in the session
    // (transcription) gets an indicator, and the outcome gets a state.
    let _in_process = InProcessSession::start();
    let injection = (!skip_inject()).then(|| {
        let injector = LinuxInjector::new();
        let target = injector.focus();
        (injector, target)
    });
    let meter = audio::process_meter();
    let hud = crate::ui::hud::RecordingHud::start(meter.clone());
    let (capture, started_at) =
        match capture_with_started_at(SystemTime::now, || capture_pcm(&mut stop, limit, &meter)) {
            Ok(capture) => capture,
            Err(reason) => {
                hud.set_state(crate::ui::hud::HudState::Failed);
                let _ = session.fail(reason);
                log_state(&session);
                let _ = status::write_status(session.state(), None, None, None);
                crate::notify::notify_session_failure(reason, None);
                return 1;
            }
        };
    stop.clear_stop_request();
    hud.set_state(crate::ui::hud::HudState::Transcribing);
    apply_edge(&mut session, HotkeyEvent::Up);
    let _ = status::write_status(session.state(), None, None, None);

    let (dict, dictionary_warning) = dictionary_for_transcription(Dictionary::load());
    let mut persistence_warnings = Vec::new();
    if let Some(warning) = dictionary_warning {
        eprintln!("{warning}");
        let _ = status::write_status(session.state(), None, Some(&warning), None);
        crate::notify::notify_persistence_failure(&warning);
        persistence_warnings.push(warning);
    }
    let prepared = match crate::transcribe::prepare_with_config(
        crate::transcribe::RunOverrides::default(),
        config,
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
            let visible_detail = joined_details(&persistence_warnings, Some(&detail));
            let _ = status::write_status(session.state(), None, visible_detail.as_deref(), None);
            eprintln!("{detail}");
            crate::notify::notify_session_failure(reason, Some(&detail));
            return 1;
        }
    };
    let transcript = match prepared.transcribe_bounded(
        &capture.pcm,
        crate::transcribe::TranscriptionPurpose::Dictation(&dict),
        Instant::now() + Duration::from_secs(15 * 60),
        &|| stop.stop_requested(),
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
            let visible_detail = joined_details(&persistence_warnings, detail);
            let _ = status::write_status(session.state(), None, visible_detail.as_deref(), None);
            crate::notify::notify_session_failure(reason, detail);
            return 1;
        }
    };

    if session.begin_injecting().is_ok() {
        log_state(&session);
    }

    let inject = match injection {
        None => InjectReport::ClipboardOnly,
        Some((injector, Ok(target))) => injector.inject(&transcript.text, &target),
        Some((_, Err(reason))) => InjectReport::Failed { reason },
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

    let history_id = new_history_id();
    let history_result = History::append_default(HistoryRow {
        id: history_id.clone(),
        text: transcript.text.clone(),
        raw: transcript.raw.clone(),
        engine: transcript.engine.clone(),
        started_at,
        infer_ms: transcript.infer_ms,
        inject,
        detail: transcript.detail.clone(),
    });
    let persisted_history_id = history_result.is_ok().then_some(history_id);
    if let Some(warning) = history_append_warning(history_result) {
        eprintln!("{warning}");
        crate::notify::notify_persistence_failure(&warning);
        persistence_warnings.push(warning);
    }
    let persistence_detail = joined_details(&persistence_warnings, None);
    if failed {
        // Leave the Failed state visible; the next session overwrites it.
        let _ = status::write_status(
            session.state(),
            Some(&transcript.text),
            persistence_detail.as_deref(),
            persisted_history_id.as_deref(),
        );
        return 1;
    }
    let _ = status::write_status(
        session.state(),
        Some(&transcript.text),
        persistence_detail.as_deref(),
        persisted_history_id.as_deref(),
    );
    0
}

fn capture_with_started_at<T, E>(
    now: impl FnOnce() -> SystemTime,
    capture: impl FnOnce() -> Result<T, E>,
) -> Result<(T, u64), E> {
    let started_at = now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    capture().map(|capture| (capture, started_at))
}

fn new_history_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn dictionary_for_transcription(
    result: Result<Dictionary, String>,
) -> (Dictionary, Option<String>) {
    match result {
        Ok(dictionary) => (dictionary, None),
        Err(error) => (
            Dictionary::empty(),
            Some(crate::notify::dictionary_read_failure_message(&error)),
        ),
    }
}

fn history_append_warning(result: Result<(), String>) -> Option<String> {
    result
        .err()
        .map(|error| crate::notify::history_append_failure_message(&error))
}

fn joined_details(warnings: &[String], detail: Option<&str>) -> Option<String> {
    let mut details = warnings.to_vec();
    if let Some(detail) = detail.filter(|detail| !detail.trim().is_empty()) {
        details.push(detail.to_string());
    }
    (!details.is_empty()).then(|| details.join(" "))
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
    play_fixture_capture_with_player(capture, stop, limit, meter, audio::play_fixture_meter)
}

fn play_fixture_capture_with_player(
    capture: audio::CaptureResult,
    stop: &StopWhen,
    limit: RecordingLimit,
    meter: &audio::LevelMeter,
    play: impl FnOnce(
        &Pcm16kMono,
        audio::LevelMeter,
        CancellationToken,
    ) -> std::thread::JoinHandle<usize>,
) -> Result<audio::CaptureResult, FailReason> {
    let max_samples = (limit.seconds() as usize)
        .saturating_mul(SAMPLE_RATE_HZ as usize)
        .min(capture.pcm.len());
    let pcm = Pcm16kMono::from_samples(capture.pcm.samples()[..max_samples].to_vec());
    let cancel = CancellationToken::new();

    let played = std::thread::scope(|scope| {
        spawn_toggle_stop_watcher(scope, stop, cancel.clone());
        let player = play(&pcm, meter.clone(), cancel.clone());
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
    if let Some(toggle) = stop.session() {
        if toggle.stop_requested() {
            cancel.cancel();
            return;
        }
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
    directory: PrivateDir,
    _gate: std::fs::File,
    token: String,
}

pub struct RecordingSession(ToggleSession);

impl RecordingSession {
    pub fn acquire() -> Result<Self, String> {
        Self::acquire_in(&echo_core::data_dir())
    }

    fn acquire_in(dir: &Path) -> Result<Self, String> {
        match ToggleSession::acquire_in(dir)? {
            LockAcquisition::Started(session) => Ok(Self(session)),
            LockAcquisition::Busy(_) => Err("Another recording is already active.".to_string()),
        }
    }

    #[must_use]
    pub fn stop_requested(&self) -> bool {
        self.0.stop_requested()
    }

    pub fn clear_stop_request(&self) {
        self.0.clear_stop_request();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LockOwner {
    pid: u32,
    token: Option<String>,
    start_time_ticks: Option<u64>,
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
            if let Ok(directory) = PrivateDir::open(dir) {
                let _ = directory.remove_file("recording.lock".as_ref());
                let _ = directory.remove_file("recording.stop".as_ref());
            }
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
        let directory = PrivateDir::open(dir).map_err(|err| err.to_string())?;
        let gate = directory
            .open_or_create("recording.gate".as_ref())
            .map_err(|err| err.to_string())?;
        match gate.try_lock_exclusive() {
            Ok(()) => {}
            Err(err) if err.kind() == ErrorKind::WouldBlock => {
                for _ in 0..32 {
                    if let Some(owner) = live_lock_owner(&dir.join("recording.lock")) {
                        return Ok(LockAcquisition::Busy(owner));
                    }
                    std::thread::yield_now();
                }
                return Err("another recording is starting".to_string());
            }
            Err(err) => return Err(err.to_string()),
        }
        let process = read_process_observation(std::process::id())
            .ok_or_else(|| "cannot read recording owner process identity".to_string())?;
        if process.state == 'Z' {
            return Err("recording owner process is a zombie".to_string());
        }
        for _ in 0..2 {
            let token = new_session_token();
            let candidate = format!(".recording.lock.{token}");
            match directory.create_new(candidate.as_ref()) {
                Ok(mut lock) => {
                    writeln!(
                        lock,
                        "{}\n{token}\n{}",
                        std::process::id(),
                        process.start_time_ticks
                    )
                    .map_err(|err| err.to_string())?;
                    drop(lock);
                    match directory.hard_link(candidate.as_ref(), "recording.lock".as_ref()) {
                        Ok(()) => {}
                        Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                            let _ = directory.remove_file(candidate.as_ref());
                            if let Some(owner) = live_lock_owner(&dir.join("recording.lock")) {
                                return Ok(LockAcquisition::Busy(owner));
                            }
                            let _ = directory.remove_file("recording.lock".as_ref());
                            let _ = directory.remove_file("recording.stop".as_ref());
                            continue;
                        }
                        Err(err) => {
                            let _ = directory.remove_file(candidate.as_ref());
                            return Err(err.to_string());
                        }
                    }
                    let _ = directory.remove_file(candidate.as_ref());
                    return Ok(LockAcquisition::Started(Self {
                        directory,
                        _gate: gate,
                        token,
                    }));
                }
                Err(err) => return Err(err.to_string()),
            }
        }
        Err("could not acquire recording lock".to_string())
    }

    fn stop_requested(&self) -> bool {
        self.directory
            .read_to_string("recording.stop".as_ref())
            .ok()
            .is_some_and(|token| token.trim() == self.token)
    }

    fn clear_stop_request(&self) {
        if self.stop_requested() {
            let _ = self.directory.remove_file("recording.stop".as_ref());
        }
    }
}

impl Drop for ToggleSession {
    fn drop(&mut self) {
        let still_owned = self
            .directory
            .read_to_string("recording.lock".as_ref())
            .ok()
            .and_then(|raw| parse_lock_owner(&raw))
            .is_some_and(|owner| owner.token.as_deref() == Some(self.token.as_str()));
        if still_owned {
            let _ = self.directory.remove_file("recording.stop".as_ref());
            let _ = self.directory.remove_file("recording.lock".as_ref());
        }
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

#[cfg(test)]
fn lock_owner(path: &Path) -> Option<LockOwner> {
    let directory = PrivateDir::open(path.parent()?).ok()?;
    let raw = directory.read_to_string(path.file_name()?).ok()?;
    parse_lock_owner(&raw)
}

fn parse_lock_owner(raw: &str) -> Option<LockOwner> {
    let mut lines = raw.lines();
    let pid = lines.next()?.trim().parse().ok()?;
    let token = lines
        .next()
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string);
    let start_time_ticks = lines
        .next()
        .map(str::trim)
        .filter(|identity| !identity.is_empty())
        .map(str::parse)
        .transpose()
        .ok()?;
    Some(LockOwner {
        pid,
        token,
        start_time_ticks,
    })
}

fn live_lock_owner(path: &Path) -> Option<LockOwner> {
    let directory = PrivateDir::open(path.parent()?).ok()?;
    let raw = directory.read_to_string(path.file_name()?).ok()?;
    live_lock_owner_from_at(&raw, lock_timestamp(path), read_process_observation)
}

fn lock_owner_is_alive(path: &Path) -> bool {
    live_lock_owner(path).is_some()
}

fn write_stop_request(path: &Path, owner: &LockOwner) -> Result<(), String> {
    let contents = owner.token.as_deref().unwrap_or("stop");
    echo_core::write_atomic_private(path, format!("{contents}\n").as_bytes())
}

#[cfg(test)]
fn live_lock_owner_from(
    raw: &str,
    observe: impl FnOnce(u32) -> Option<ProcessObservation>,
) -> Option<LockOwner> {
    live_lock_owner_from_at(raw, None, observe)
}

fn live_lock_owner_from_at(
    raw: &str,
    fallback_acquired_at: Option<u128>,
    observe: impl FnOnce(u32) -> Option<ProcessObservation>,
) -> Option<LockOwner> {
    let owner = parse_lock_owner(raw)?;
    let process = observe(owner.pid)?;
    if process.state == 'Z' {
        return None;
    }
    match owner.start_time_ticks {
        Some(recorded) if recorded == process.start_time_ticks => Some(owner),
        Some(_) => None,
        None => {
            let acquired_at = legacy_token_timestamp(&owner).or(fallback_acquired_at)?;
            let started_at = process.start_unix_nanos?;
            (started_at <= acquired_at).then_some(owner)
        }
    }
}

fn lock_timestamp(path: &Path) -> Option<u128> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if !metadata.file_type().is_file() {
        return None;
    }
    metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|time| time.as_nanos())
}

fn legacy_token_timestamp(owner: &LockOwner) -> Option<u128> {
    let token = owner.token.as_deref()?;
    let mut fields = token.split('-');
    let token_pid = fields.next()?.parse::<u32>().ok()?;
    let seconds = fields.next()?.parse::<u128>().ok()?;
    let nanos = fields.next()?.parse::<u128>().ok()?;
    let _sequence = fields.next()?.parse::<u64>().ok()?;
    if token_pid != owner.pid || nanos >= 1_000_000_000 || fields.next().is_some() {
        return None;
    }
    seconds.checked_mul(1_000_000_000)?.checked_add(nanos)
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
    let (config, _) = crate::settings::config_for_display();
    echo_core::resolve_recording_limit(environment.as_deref(), config.record_seconds)
}

fn fixture_path() -> Option<PathBuf> {
    std::env::var_os("ECHO_AUDIO_FIXTURE").map(PathBuf::from)
}

fn log_state(session: &Session) {
    let name = match session.state() {
        SessionState::Idle => "Idle",
        SessionState::Recording { .. } => "Recording",
        SessionState::Transcribing => "Transcribing",
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
    use std::cell::Cell;
    use std::fs;
    use std::sync::{mpsc, Arc, Barrier};

    fn observed(state: char, start_time_ticks: u64, start_unix_nanos: u128) -> ProcessObservation {
        ProcessObservation {
            pid: 41,
            state,
            start_time_ticks,
            start_unix_nanos: Some(start_unix_nanos),
        }
    }

    #[test]
    fn dictionary_read_failure_is_visible_without_discarding_the_transcript() {
        let (dictionary, warning) =
            dictionary_for_transcription(Err("dictionary permission denied".to_string()));
        let warning = warning.expect("dictionary failure should be reported");
        assert!(dictionary.entries().is_empty());

        let transcript = "Keep résumé text";
        let body = status::render(SessionState::Idle, Some(transcript), Some(&warning), None);
        assert!(body.contains("state=Idle\n"), "{body}");
        assert!(body.contains("last=Keep résumé text\n"), "{body}");
        assert!(body.contains("custom replacements were skipped"), "{body}");
        assert!(body.contains("Echo → Dictionary"), "{body}");
    }

    #[test]
    fn malformed_dictionary_reaches_the_visible_warning_path() {
        let dir = std::env::temp_dir().join(format!(
            "echo-rec-malformed-dictionary-{}-{}",
            std::process::id(),
            new_session_token()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dictionary.json");
        fs::write(&path, "{\"entries\": [").unwrap();

        let (dictionary, warning) = dictionary_for_transcription(Dictionary::load_from(&path));
        let warning = warning.expect("malformed dictionary should be reported");

        assert!(dictionary.entries().is_empty());
        assert!(
            warning.contains("custom replacements were skipped"),
            "{warning}"
        );
        assert!(warning.contains("invalid JSON"), "{warning}");
        assert!(warning.contains(path.to_str().unwrap()), "{warning}");
        assert_eq!(
            fs::read_to_string(dir.join("dictionary.json.corrupt")).unwrap(),
            "{\"entries\": ["
        );
        assert!(!path.exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn history_append_failure_is_visible_and_not_described_as_persisted() {
        let warning = history_append_warning(Err("read-only file system".to_string()))
            .expect("history failure should be reported");
        let body = status::render(
            SessionState::Idle,
            Some("still recoverable"),
            Some(&warning),
            None,
        );

        assert!(body.contains("last=still recoverable\n"), "{body}");
        assert!(
            body.contains("couldn't save the transcript to history"),
            "{body}"
        );
        assert!(body.contains("read-only file system"), "{body}");
        assert!(
            body.contains("check the data directory permissions"),
            "{body}"
        );
    }

    #[test]
    fn history_started_at_persists_capture_time_across_calendar_boundary() {
        const BEFORE_MIDNIGHT: u64 = 86_399;
        const AFTER_MIDNIGHT: u64 = 86_401;
        let clock = Cell::new(BEFORE_MIDNIGHT);
        let (_, started_at) = capture_with_started_at(
            || UNIX_EPOCH + Duration::from_secs(clock.get()),
            || {
                clock.set(AFTER_MIDNIGHT);
                Ok::<(), ()>(())
            },
        )
        .unwrap();

        let dir = std::env::temp_dir().join(format!(
            "echo-rec-started-at-{}-{}",
            std::process::id(),
            new_session_token()
        ));
        let path = dir.join("history.json");
        let mut history = History::load_from(&path).unwrap();
        history
            .append(HistoryRow {
                id: "cross-midnight".to_string(),
                text: "captured before midnight".to_string(),
                raw: "captured before midnight".to_string(),
                engine: echo_core::EngineId::Whisper {
                    model: "test".to_string(),
                },
                started_at,
                infer_ms: 1,
                inject: InjectReport::ClipboardOnly,
                detail: echo_core::RunDetail::default(),
            })
            .unwrap();

        let reloaded = History::load_from(&path).unwrap();
        assert_eq!(clock.get(), AFTER_MIDNIGHT);
        assert_eq!(reloaded.rows()[0].started_at, BEFORE_MIDNIGHT);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn history_ids_are_uuid_v4_and_unique() {
        let ids = (0..1_000)
            .map(|_| new_history_id())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(ids.len(), 1_000);
        assert!(ids
            .iter()
            .all(|id| { uuid::Uuid::parse_str(id).is_ok_and(|uuid| uuid.get_version_num() == 4) }));
    }

    #[test]
    fn process_stat_parser_handles_spaces_and_parentheses_in_comm() {
        let mut trailing = vec!["0"; 18];
        trailing.push("424242");
        let raw = format!("77 (echo worker (old)) S {}", trailing.join(" "));
        assert_eq!(
            crate::process_identity::parse_stat(&raw),
            Some(('S', 424242))
        );
    }

    #[test]
    fn new_lock_accepts_a_live_matching_process_identity() {
        let raw = "41\n41-200-123-0\n9001\n";
        assert!(live_lock_owner_from(raw, |_| Some(observed('S', 9001, 100))).is_some());
    }

    #[test]
    fn new_lock_rejects_a_zombie_owner() {
        let raw = "41\n41-200-123-0\n9001\n";
        assert!(live_lock_owner_from(raw, |_| Some(observed('Z', 9001, 100))).is_none());
    }

    #[test]
    fn new_lock_rejects_a_reused_pid_with_a_different_start_identity() {
        let raw = "41\n41-200-123-0\n9001\n";
        assert!(
            live_lock_owner_from(raw, |_| Some(observed('S', 9002, 300_000_000_000))).is_none(),
            "a reused pid has a different field-22 start identity"
        );
    }

    #[test]
    fn legacy_two_line_lock_requires_process_to_predate_token() {
        let raw = "41\n41-200-123-0\n";
        let acquired = 200_000_000_123_u128;
        assert!(
            live_lock_owner_from(raw, |_| Some(observed('S', 9001, acquired - 1))).is_some(),
            "a currently-running legacy owner remains protected"
        );
        assert!(
            live_lock_owner_from(raw, |_| Some(observed('S', 9002, acquired + 1))).is_none(),
            "a process started after the token cannot inherit a legacy lock"
        );
        assert!(
            live_lock_owner_from(raw, |_| Some(ProcessObservation {
                pid: 41,
                state: 'S',
                start_time_ticks: 9001,
                start_unix_nanos: None,
            }))
            .is_none(),
            "legacy validation fails closed when start time cannot be resolved"
        );
    }

    #[test]
    fn takeover_boundary_blocks_acquisition_and_spawn_failure_reopens_it() {
        let dir = std::env::temp_dir().join(format!(
            "echo-takeover-failure-{}-{}",
            std::process::id(),
            new_session_token()
        ));
        let outcome = attempt_upgrade_takeover_in(&dir, || {
            assert!(ToggleSession::try_start_in(&dir).unwrap().is_none());
            Err(std::io::Error::new(
                ErrorKind::NotFound,
                "replacement missing",
            ))
        });
        assert!(matches!(outcome, UpgradeTakeover::SpawnFailed(_)));
        assert!(ToggleSession::try_start_in(&dir).unwrap().is_some());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn takeover_reservation_blocks_recording_until_released() {
        let dir = std::env::temp_dir().join(format!(
            "echo-takeover-success-{}-{}",
            std::process::id(),
            new_session_token()
        ));
        let reservation = reserve_upgrade_takeover_in(&dir).unwrap();
        assert!(ToggleSession::try_start_in(&dir).unwrap().is_none());
        drop(reservation);
        assert!(ToggleSession::try_start_in(&dir).unwrap().is_some());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn active_acquisition_defers_takeover_without_spawning() {
        let dir = std::env::temp_dir().join(format!(
            "echo-takeover-busy-{}-{}",
            std::process::id(),
            new_session_token()
        ));
        let session = match ToggleSession::acquire_in(&dir).unwrap() {
            LockAcquisition::Started(session) => session,
            LockAcquisition::Busy(_) => panic!("test directory should be idle"),
        };
        let mut spawned = false;
        let outcome = attempt_upgrade_takeover_in(&dir, || {
            spawned = true;
            Ok(())
        });
        assert!(matches!(outcome, UpgradeTakeover::Deferred));
        assert!(!spawned);
        drop(session);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn stale_reclaim_admits_exactly_one_concurrent_recording() {
        let dir = std::env::temp_dir().join(format!(
            "echo-stale-reclaim-{}-{}",
            std::process::id(),
            new_session_token()
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("recording.lock"), "99999999\nstale\n1\n").unwrap();
        let barrier = Arc::new(Barrier::new(3));
        let contenders = (0..2)
            .map(|_| {
                let dir = dir.clone();
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    ToggleSession::try_start_in(&dir)
                })
            })
            .collect::<Vec<_>>();
        barrier.wait();
        let sessions = contenders
            .into_iter()
            .map(|contender| contender.join().unwrap().unwrap())
            .collect::<Vec<_>>();

        assert_eq!(
            sessions.iter().filter(|session| session.is_some()).count(),
            1
        );
        drop(sessions);
        let _ = fs::remove_dir_all(dir);
    }

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

    #[cfg(unix)]
    #[test]
    fn recording_lock_secures_its_directory_and_file() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!(
            "echo-recording-private-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let session = ToggleSession::try_start_in(&dir).unwrap().unwrap();

        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(dir.join("recording.lock"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let owner = lock_owner(&dir.join("recording.lock")).unwrap();
        assert_eq!(
            owner.start_time_ticks,
            Some(
                read_process_observation(std::process::id())
                    .unwrap()
                    .start_time_ticks
            ),
            "new on-disk locks include the owner's /proc start identity"
        );

        drop(session);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn token_scoped_stop_ignores_an_unrelated_session() {
        let dir = std::env::temp_dir().join(format!("echo-scoped-stop-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let session = ToggleSession::try_start_in(&dir).unwrap().unwrap();

        assert!(!ToggleSession::request_stop_for_token_in(&dir, "another-session").unwrap());
        assert!(!dir.join("recording.stop").exists());
        assert!(ToggleSession::request_stop_for_token_in(&dir, &session.token).unwrap());
        assert!(session.stop_requested());
    }

    #[test]
    fn stale_stop_request_cannot_cancel_a_replacement_session() {
        let dir = std::env::temp_dir().join(format!("echo-stop-token-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);

        let first = ToggleSession::try_start_in(&dir).unwrap().unwrap();
        let first_owner = lock_owner(&dir.join("recording.lock")).unwrap();
        drop(first);
        let second = ToggleSession::try_start_in(&dir).unwrap().unwrap();

        write_stop_request(&dir.join("recording.stop"), &first_owner).unwrap();
        assert!(!second.stop_requested());
        assert_ne!(first_owner.token.as_deref(), Some(second.token.as_str()));
    }

    #[test]
    fn a_new_stop_request_can_cancel_transcription_after_capture_stop_is_cleared() {
        let dir = std::env::temp_dir().join(format!(
            "echo-transcription-stop-{}-{}",
            std::process::id(),
            new_session_token()
        ));
        let session = ToggleSession::try_start_in(&dir).unwrap().unwrap();

        assert!(ToggleSession::request_stop_if_active_in(&dir).unwrap());
        assert!(session.stop_requested());
        session.clear_stop_request();
        assert!(!session.stop_requested());
        assert!(ToggleSession::request_stop_if_active_in(&dir).unwrap());
        assert!(session.stop_requested());

        drop(session);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn fixture_returns_wav_without_opening_host() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude_code.wav");
        let mut stop = StopWhen::Timer(None);
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
        let mut stop = StopWhen::Timer(None);
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
            &StopWhen::Timer(None),
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
        let full_duration = capture.duration;
        let meter = audio::LevelMeter::new();
        let (started_send, started_receive) = mpsc::sync_channel(0);
        let stopper = std::thread::spawn({
            let dir = dir.clone();
            move || {
                started_receive.recv().expect("fixture player start");
                ToggleSession::request_stop_if_active_in(&dir).unwrap()
            }
        });

        let result = play_fixture_capture_with_player(
            capture,
            &StopWhen::ToggleFile(session),
            RecordingLimit::MAX,
            &meter,
            |pcm, _meter, cancel| {
                let partial_len = (SAMPLE_RATE_HZ as usize / 33).min(pcm.len());
                std::thread::spawn(move || {
                    started_send.send(()).expect("fixture start receiver");
                    while !cancel.is_cancelled() {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    partial_len
                })
            },
        )
        .unwrap();

        assert!(stopper.join().expect("stopper thread"));
        assert!(result.duration > Duration::ZERO);
        assert!(result.duration < full_duration);
    }

    #[test]
    fn fixture_obeys_an_already_present_token_scoped_toggle_stop() {
        let dir =
            std::env::temp_dir().join(format!("echo-fixture-present-stop-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let session = ToggleSession::try_start_in(&dir).unwrap().unwrap();
        let pcm = Pcm16kMono::from_samples(vec![i16::MAX / 4; SAMPLE_RATE_HZ as usize * 2]);
        let capture = audio::CaptureResult::from_pcm(pcm);
        assert!(ToggleSession::request_stop_if_active_in(&dir).unwrap());

        let result = play_fixture_capture(
            capture,
            &StopWhen::ToggleFile(session),
            RecordingLimit::MAX,
            &audio::LevelMeter::new(),
        )
        .unwrap();

        assert_eq!(result.duration, Duration::ZERO);
    }

    #[test]
    fn toggle_starts_stops_and_can_restart() {
        let dir = std::env::temp_dir().join(format!("echo-toggle-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);

        let first = match ToggleSession::start_or_stop_in(&dir).unwrap() {
            ToggleAction::Start(session) => session,
            ToggleAction::Stop(_) => panic!("first toggle should start"),
        };
        assert!(dir.join("recording.lock").is_file());
        assert!(matches!(
            ToggleSession::start_or_stop_in(&dir).unwrap(),
            ToggleAction::Stop(_)
        ));
        assert!(dir.join("recording.stop").is_file());

        drop(first);
        assert!(!dir.join("recording.lock").exists());
        assert!(!dir.join("recording.stop").exists());
        assert!(matches!(
            ToggleSession::start_or_stop_in(&dir).unwrap(),
            ToggleAction::Start(_)
        ));
    }

    #[test]
    fn recording_session_serializes_with_toggle_recording() {
        let dir = std::env::temp_dir().join(format!(
            "echo-shared-recording-session-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        let session = RecordingSession::acquire_in(&dir).unwrap();

        assert!(ToggleSession::try_start_in(&dir).unwrap().is_none());
        assert!(ToggleSession::request_stop_if_active_in(&dir).unwrap());
        assert!(session.stop_requested());

        drop(session);
        assert!(ToggleSession::try_start_in(&dir).unwrap().is_some());
    }
}
