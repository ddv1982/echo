use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use echo_core::{
    Dictionary, Engine, FailReason, History, HistoryRow, InjectReport, Injector, Session,
    SessionState,
};

use crate::audio::{self, AudioCapture};
use crate::hotkey::{self, HotkeyEvent, HotkeySource};
use crate::inject::LinuxInjector;
use crate::stt::{FakeEngine, ParakeetEngine, WhisperEngine};
use crate::ui::tray;

pub fn run_rec_once() -> i32 {
    let mut session = Session::new();
    log_state(&session);
    let _ = tray::write_status(session.state(), None);
    if matches!(HotkeySource::detect(), HotkeySource::Cli) {
        eprintln!("{}", hotkey::evdev_permission_hint());
    }
    apply_edge(&mut session, HotkeyEvent::Down);
    let _ = tray::write_status(session.state(), None);
    let capture = match capture_pcm() {
        Ok(capture) => capture,
        Err(reason) => {
            let _ = session.fail(reason);
            log_state(&session);
            let _ = tray::write_status(session.state(), None);
            return 1;
        }
    };
    apply_edge(&mut session, HotkeyEvent::Up);
    let _ = tray::write_status(session.state(), None);

    let engine = selected_engine();
    let transcript = match engine.transcribe(&capture.pcm) {
        Ok(t) => t,
        Err(err) => {
            let reason = match err {
                echo_core::EngineError::Missing => FailReason::EngineMissing,
                echo_core::EngineError::Infer(_) => FailReason::EngineError,
            };
            let _ = session.fail(reason);
            log_state(&session);
            let _ = tray::write_status(session.state(), None);
            return 1;
        }
    };

    if session.begin_cleaning().is_ok() {
        log_state(&session);
    }
    let dict = Dictionary::load().unwrap_or_else(|_| Dictionary::empty());
    let rewrite = dict.rewrite(&transcript.raw);
    if session.begin_injecting().is_ok() {
        log_state(&session);
    }

    let inject = if skip_inject() {
        InjectReport::ClipboardOnly
    } else {
        let injector = LinuxInjector::new();
        match injector.focus() {
            Ok(target) => injector.inject(&rewrite.text, &target),
            Err(reason) => InjectReport::Failed { reason },
        }
    };
    if inject.failed() {
        let reason = match &inject {
            InjectReport::Failed { reason } => *reason,
            _ => FailReason::InjectUnconfirmed,
        };
        let _ = session.fail(reason);
        log_state(&session);
        let _ = session.ack();
        log_state(&session);
    } else if session.complete_inject().is_ok() {
        log_state(&session);
    }

    let started_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Ok(mut history) = History::load() {
        let _ = history.append(HistoryRow {
            id: format!("{started_at}-{}", history.rows().len() + 1),
            text: rewrite.text.clone(),
            raw: transcript.raw.clone(),
            engine: transcript.engine.clone(),
            started_at,
            infer_ms: transcript.infer_ms,
            inject,
        });
    }
    let _ = tray::write_status(session.state(), Some(&rewrite.text));
    0
}

fn selected_engine() -> Box<dyn Engine> {
    match std::env::var("ECHO_ENGINE").ok().as_deref() {
        Some("whisper") => Box::new(WhisperEngine::new()),
        Some("parakeet") => Box::new(ParakeetEngine::new()),
        Some("fake") => Box::new(FakeEngine::default()),
        _ => {
            let parakeet = ParakeetEngine::new();
            if parakeet
                .transcribe(&echo_core::Pcm16kMono::from_samples(vec![0; 8]))
                .is_ok()
            {
                return Box::new(parakeet);
            }
            let whisper = WhisperEngine::new();
            if whisper
                .transcribe(&echo_core::Pcm16kMono::from_samples(vec![0; 8]))
                .is_ok()
            {
                return Box::new(whisper);
            }
            Box::new(FakeEngine::default())
        }
    }
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

fn capture_pcm() -> Result<audio::CaptureResult, FailReason> {
    if let Some(path) = fixture_path() {
        return audio::load_wav(&path).map_err(|_| FailReason::EngineError);
    }
    let capture = AudioCapture::open_default().map_err(|_| FailReason::MicPermission)?;
    let cancel = capture.cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(400));
        cancel.cancel();
    });
    capture
        .record(Duration::from_secs(2))
        .map_err(|_| FailReason::MicPermission)
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
