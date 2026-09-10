use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use echo_core::{
    Dictionary, FailReason, History, HistoryRow, InjectReport, Injector, Pcm16kMono,
    RecordingLimit, ResolvedRecordingLimit, Session, SessionState, SAMPLE_RATE_HZ,
};

use crate::audio::{self, AudioCapture, CancellationToken};
use crate::inject::LinuxInjector;
use crate::status;

use super::lease::{InProcessSession, ToggleSession};

pub(super) enum StopWhen {
    Timer(Option<ToggleSession>),
    ToggleFile(ToggleSession),
}

impl StopWhen {
    pub(super) fn session(&self) -> Option<&ToggleSession> {
        match self {
            Self::Timer(session) => session.as_ref(),
            Self::ToggleFile(session) => Some(session),
        }
    }

    pub(super) fn cancel_requested(&self) -> bool {
        self.session().is_some_and(ToggleSession::cancel_requested)
    }

    pub(super) fn session_id(&self) -> Option<&str> {
        self.session().map(|session| session.token.as_str())
    }

    pub(super) fn next_revision(&self) -> u64 {
        self.session().map_or(0, ToggleSession::next_revision)
    }
}

pub(super) struct PublishedSession {
    session: Session,
    stop: StopWhen,
    status_path: PathBuf,
}

impl PublishedSession {
    pub(super) fn new(stop: StopWhen) -> Self {
        Self::new_in(stop, echo_core::status_path())
    }

    pub(super) fn new_in(stop: StopWhen, status_path: PathBuf) -> Self {
        Self {
            session: Session::new(),
            stop,
            status_path,
        }
    }

    pub(super) fn start_recording(&mut self, limit: RecordingLimit) -> Result<u64, String> {
        self.session
            .start_recording()
            .map_err(|err| err.to_string())?;
        log_state(&self.session);
        self.publish_recording(limit)
    }

    pub(super) fn finish_capture(&mut self) -> Result<(), String> {
        self.session
            .finish_recording()
            .map_err(|err| err.to_string())?;
        log_state(&self.session);
        self.publish_state(None, None, None).map(|_| ())
    }

    pub(super) fn begin_injecting_then<T>(
        &mut self,
        effect: impl FnOnce() -> T,
    ) -> Result<Option<T>, String> {
        self.session
            .begin_injecting()
            .map_err(|err| err.to_string())?;
        log_state(&self.session);
        self.publish_state(None, None, None)?;
        // A cancel acknowledgment requires a Transcribing read after its intent
        // write. Publishing Injecting first makes every acknowledged intent
        // visible to this final check before the effect can run.
        if self.cancel_requested() {
            self.fail(
                FailReason::EngineError,
                None,
                Some("Transcription canceled"),
                None,
            )?;
            return Ok(None);
        }
        Ok(Some(effect()))
    }

    pub(super) fn complete_without_insertion(
        &mut self,
        last: Option<&str>,
        error: Option<&str>,
        last_history_id: Option<&str>,
    ) -> Result<(), String> {
        self.session
            .skip_insertion()
            .map_err(|err| err.to_string())?;
        log_state(&self.session);
        self.publish_current(last, error, last_history_id)
    }
    pub(super) fn complete_inject(
        &mut self,
        last: Option<&str>,
        error: Option<&str>,
        last_history_id: Option<&str>,
    ) -> Result<(), String> {
        self.session
            .complete_inject()
            .map_err(|err| err.to_string())?;
        log_state(&self.session);
        self.publish_current(last, error, last_history_id)
    }

    pub(super) fn fail(
        &mut self,
        reason: FailReason,
        last: Option<&str>,
        error: Option<&str>,
        last_history_id: Option<&str>,
    ) -> Result<(), String> {
        self.session.fail(reason).map_err(|err| err.to_string())?;
        log_state(&self.session);
        self.publish_current(last, error, last_history_id)
    }

    pub(super) fn publish_current(
        &self,
        last: Option<&str>,
        error: Option<&str>,
        last_history_id: Option<&str>,
    ) -> Result<(), String> {
        self.publish_state(last, error, last_history_id).map(|_| ())
    }

    pub(super) fn cancel_requested(&self) -> bool {
        self.stop.cancel_requested()
    }

    pub(super) fn publish_recording(&self, limit: RecordingLimit) -> Result<u64, String> {
        match self.stop.session_id() {
            Some(session_id) => {
                let revision = self.stop.next_revision();
                status::write_recording_for_session_at(
                    &self.status_path,
                    session_id,
                    revision,
                    limit,
                )?;
                Ok(revision)
            }
            None => {
                status::write_recording(limit)?;
                Ok(0)
            }
        }
    }

    pub(super) fn publish_state(
        &self,
        last: Option<&str>,
        error: Option<&str>,
        last_history_id: Option<&str>,
    ) -> Result<u64, String> {
        match self.stop.session_id() {
            Some(session_id) => {
                let revision = self.stop.next_revision();
                status::write_status_for_session_at(
                    &self.status_path,
                    (session_id, revision),
                    self.session.state(),
                    last,
                    error,
                    last_history_id,
                )?;
                Ok(revision)
            }
            None => {
                status::write_status(self.session.state(), last, error, last_history_id)?;
                Ok(0)
            }
        }
    }
}

pub(super) fn run_record(stop: StopWhen) -> i32 {
    run_record_started(stop, None)
}

pub(super) fn run_record_started(
    stop: StopWhen,
    started: Option<std::sync::mpsc::SyncSender<Result<u64, String>>>,
) -> i32 {
    let config = match crate::settings::runtime_config() {
        Ok(config) => config,
        Err(error) => {
            if let Some(sender) = started {
                let _ = sender.send(Err(error.clone()));
            }
            eprintln!("{error}");
            if let Err(err) = publish_startup_failure(&stop, &error) {
                report_publication_failure(&err);
            }
            crate::notify::notify_session_failure(FailReason::EngineError, Some(&error));
            return 1;
        }
    };
    let environment = std::env::var("ECHO_RECORD_SECONDS").ok();
    let limit =
        echo_core::resolve_recording_limit(environment.as_deref(), config.record_seconds).limit;
    run_record_with_limit(stop, limit, &config, started)
}

pub(super) fn run_record_with_limit(
    stop: StopWhen,
    limit: RecordingLimit,
    config: &echo_core::Config,
    started: Option<std::sync::mpsc::SyncSender<Result<u64, String>>>,
) -> i32 {
    let mut published = PublishedSession::new(stop);
    let initial = published.start_recording(limit);
    if let Some(sender) = started {
        let _ = sender.send(initial.clone());
    }
    if let Err(err) = initial {
        report_publication_failure(&err);
        return 1;
    }
    // The HUD lives until after injection: the longest wait in the session
    // (transcription) gets an indicator, and the outcome gets a state.
    let _in_process = InProcessSession::start();
    let injector = (!skip_inject()).then(LinuxInjector::new);
    let meter = audio::process_meter();
    let hud = crate::ui::hud::RecordingHud::start(meter.clone());
    let (capture, started_at) = match capture_with_started_at(SystemTime::now, || {
        capture_pcm(&published.stop, limit, &meter)
    }) {
        Ok(capture) => capture,
        Err(failure) => {
            hud.set_state(crate::ui::hud::HudState::Failed);
            if let Err(err) = published.fail(failure.reason, None, failure.detail.as_deref(), None)
            {
                report_publication_failure(&err);
            }
            crate::notify::notify_session_failure(failure.reason, failure.detail.as_deref());
            return 1;
        }
    };
    // A Home-button start leaves Echo focused. Pin the target after capture,
    // once the user has returned to the application that should receive text,
    // and before transcription creates another opportunity for focus to move.
    let injection = injector.map(|injector| {
        let target = injector.focus();
        (injector, target)
    });
    hud.set_state(crate::ui::hud::HudState::Transcribing);
    if let Err(err) = published.finish_capture() {
        report_publication_failure(&err);
        crate::notify::notify_session_failure(FailReason::EngineError, Some(&err));
        return 1;
    }

    let (dict, dictionary_warning) = dictionary_for_transcription(Dictionary::load());
    let mut persistence_warnings = Vec::new();
    if let Some(warning) = dictionary_warning {
        eprintln!("{warning}");
        if let Err(err) = published.publish_current(None, Some(&warning), None) {
            report_publication_failure(&err);
            crate::notify::notify_persistence_failure(&warning);
            return 1;
        }
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
            let detail = err.to_string();
            let visible_detail = joined_details(&persistence_warnings, Some(&detail));
            if let Err(err) = published.fail(reason, None, visible_detail.as_deref(), None) {
                report_publication_failure(&err);
            }
            eprintln!("{detail}");
            crate::notify::notify_session_failure(reason, Some(&detail));
            return 1;
        }
    };
    let transcript = match prepared.transcribe_bounded(
        &capture.pcm,
        crate::transcribe::TranscriptionPurpose::Dictation(&dict),
        Instant::now() + Duration::from_secs(15 * 60),
        &|| published.cancel_requested(),
    ) {
        Ok(transcript) => transcript,
        Err(err) => {
            hud.set_state(crate::ui::hud::HudState::Failed);
            let (reason, message) =
                transcription_failure(&err, published.cancel_requested(), &persistence_warnings);
            let detail = Some(message.as_str());
            if let Err(err) = published.fail(reason, None, detail, None) {
                report_publication_failure(&err);
            }
            crate::notify::notify_session_failure(reason, detail);
            return 1;
        }
    };

    if is_silence(&transcript.text) {
        hud.set_state(crate::ui::hud::HudState::Done);
        let persistence_detail = joined_details(&persistence_warnings, None);
        if let Err(err) =
            published.complete_without_insertion(None, persistence_detail.as_deref(), None)
        {
            report_publication_failure(&err);
            return 1;
        }
        return 0;
    }

    let inject = match published.begin_injecting_then(|| match injection {
        None => InjectReport::ClipboardOnly,
        Some((injector, Ok(target))) => injector.inject(&transcript.text, &target),
        Some((_, Err(reason))) => InjectReport::Failed { reason },
    }) {
        Ok(Some(inject)) => inject,
        Ok(None) => {
            hud.set_state(crate::ui::hud::HudState::Failed);
            crate::notify::notify_session_failure(
                FailReason::EngineError,
                Some("Transcription canceled"),
            );
            return 1;
        }
        Err(err) => {
            report_publication_failure(&err);
            crate::notify::notify_session_failure(FailReason::EngineError, Some(&err));
            return 1;
        }
    };
    let failed = inject.failed();
    if failed {
        let reason = match &inject {
            InjectReport::Failed { reason } => *reason,
            _ => FailReason::InjectUnconfirmed,
        };
        hud.set_state(crate::ui::hud::HudState::Failed);
        if let Err(err) = published.fail(reason, None, None, None) {
            report_publication_failure(&err);
            crate::notify::notify_session_failure(FailReason::EngineError, Some(&err));
            // Insertion has already been attempted; still preserve its transcript in History.
        }
        crate::notify::notify_session_failure(reason, None);
    } else {
        hud.set_state(crate::ui::hud::HudState::Done);
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
        if let Err(err) = published.publish_current(
            Some(&transcript.text),
            persistence_detail.as_deref(),
            persisted_history_id.as_deref(),
        ) {
            report_publication_failure(&err);
        }
        return 1;
    }
    if let Err(err) = published.complete_inject(
        Some(&transcript.text),
        persistence_detail.as_deref(),
        persisted_history_id.as_deref(),
    ) {
        report_publication_failure(&err);
        return 1;
    }
    0
}

pub(super) fn transcription_failure(
    error: &crate::transcribe::TranscriptionError,
    canceled: bool,
    warnings: &[String],
) -> (FailReason, String) {
    if canceled {
        return (
            FailReason::EngineError,
            "Transcription canceled".to_string(),
        );
    }
    let reason = match error {
        crate::transcribe::TranscriptionError::Engine(echo_core::EngineError::Missing) => {
            FailReason::EngineMissing
        }
        _ => FailReason::EngineError,
    };
    let mut details = warnings.to_vec();
    details.push(error.to_string());
    (reason, details.join(" "))
}

pub(super) fn capture_with_started_at<T, E>(
    now: impl FnOnce() -> SystemTime,
    capture: impl FnOnce() -> Result<T, E>,
) -> Result<(T, u64), E> {
    let started_at = now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    capture().map(|capture| (capture, started_at))
}

pub(super) fn new_history_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub(super) fn publish_startup_failure(stop: &StopWhen, error: &str) -> Result<(), String> {
    let state = SessionState::Failed {
        reason: FailReason::EngineError,
    };
    match stop.session_id() {
        Some(session_id) => status::write_status_for_session(
            session_id,
            stop.next_revision(),
            state,
            None,
            Some(error),
            None,
        ),
        None => status::write_status(state, None, Some(error), None),
    }
}

pub(super) fn report_publication_failure(err: &str) {
    let message = format!("Echo couldn't update recording status: {err}");
    eprintln!("{message}");
    crate::notify::notify_persistence_failure(&message);
}

pub(super) fn dictionary_for_transcription(
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

pub(super) fn history_append_warning(result: Result<(), String>) -> Option<String> {
    result
        .err()
        .map(|error| crate::notify::history_append_failure_message(&error))
}

pub(super) fn joined_details(warnings: &[String], detail: Option<&str>) -> Option<String> {
    let mut details = warnings.to_vec();
    if let Some(detail) = detail.filter(|detail| !detail.trim().is_empty()) {
        details.push(detail.to_string());
    }
    (!details.is_empty()).then(|| details.join(" "))
}

pub(super) fn skip_inject() -> bool {
    matches!(
        std::env::var("ECHO_SKIP_INJECT").ok().as_deref(),
        Some("1") | Some("true")
    )
}

#[must_use]
pub(super) fn is_silence(text: &str) -> bool {
    text.trim().is_empty()
}

#[derive(Debug)]
pub(super) struct CaptureFailure {
    pub(super) reason: FailReason,
    pub(super) detail: Option<String>,
}

impl CaptureFailure {
    pub(super) fn from_audio(error: audio::AudioError) -> Self {
        let reason = match &error {
            audio::AudioError::NoDevice => FailReason::NoInputDevice,
            _ => FailReason::CaptureFailed,
        };
        Self {
            reason,
            detail: Some(error.to_string()),
        }
    }
}

pub(super) fn capture_pcm(
    stop: &StopWhen,
    limit: RecordingLimit,
    meter: &audio::LevelMeter,
) -> Result<audio::CaptureResult, CaptureFailure> {
    capture_from(fixture_path(), stop, limit, meter)
}

pub(super) fn capture_from(
    fixture: Option<PathBuf>,
    stop: &StopWhen,
    limit: RecordingLimit,
    meter: &audio::LevelMeter,
) -> Result<audio::CaptureResult, CaptureFailure> {
    if let Some(path) = fixture {
        let capture = audio::load_wav(&path).map_err(|error| CaptureFailure {
            reason: FailReason::EngineError,
            detail: Some(error.to_string()),
        })?;
        return play_fixture_capture(capture, stop, limit, meter).map_err(|reason| {
            CaptureFailure {
                reason,
                detail: None,
            }
        });
    }
    let capture = AudioCapture::open_default().map_err(CaptureFailure::from_audio)?;
    record_device(&capture, stop, limit, meter).map_err(CaptureFailure::from_audio)
}

pub(super) fn play_fixture_capture(
    capture: audio::CaptureResult,
    stop: &StopWhen,
    limit: RecordingLimit,
    meter: &audio::LevelMeter,
) -> Result<audio::CaptureResult, FailReason> {
    play_fixture_capture_with_player(capture, stop, limit, meter, audio::play_fixture_meter)
}

pub(super) fn play_fixture_capture_with_player(
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

pub(super) fn record_device(
    capture: &AudioCapture,
    stop: &StopWhen,
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

pub(super) fn spawn_toggle_stop_watcher<'scope>(
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

#[must_use]
pub fn recording_limit_from_process() -> ResolvedRecordingLimit {
    let environment = std::env::var("ECHO_RECORD_SECONDS").ok();
    let (config, _) = crate::settings::config_for_display();
    echo_core::resolve_recording_limit(environment.as_deref(), config.record_seconds)
}

pub(super) fn fixture_path() -> Option<PathBuf> {
    audio_fixture_path(
        cfg!(debug_assertions),
        std::env::var_os("ECHO_AUDIO_FIXTURE"),
    )
}

pub(super) fn audio_fixture_path(
    debug_build: bool,
    value: Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    debug_build.then(|| value.map(PathBuf::from)).flatten()
}

pub(super) fn log_state(session: &Session) {
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
