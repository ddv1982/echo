use std::path::PathBuf;
use std::time::Duration;

use echo_core::{FailReason, Session, SessionState};

use crate::audio::{self, AudioCapture};
use crate::hotkey::{self, HotkeyEvent, HotkeySource};

pub fn run_rec_once() -> i32 {
    let mut session = Session::new();
    log_state(&session);
    if matches!(HotkeySource::detect(), HotkeySource::Cli) {
        eprintln!("{}", hotkey::evdev_permission_hint());
    }
    apply_edge(&mut session, HotkeyEvent::Down);
    match capture_pcm() {
        Ok(_) => {
            apply_edge(&mut session, HotkeyEvent::Up);
            0
        }
        Err(reason) => {
            let _ = session.fail(reason);
            log_state(&session);
            1
        }
    }
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
