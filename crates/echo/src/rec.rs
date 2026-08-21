use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use echo_core::{
    Dictionary, FailReason, History, HistoryRow, InjectReport, Injector, Session, SessionState,
};

use crate::audio::{self, AudioCapture, CancellationToken};
use crate::hotkey::{self, HoldKey, HotkeyEvent, HotkeySource};
use crate::inject::LinuxInjector;
use crate::status;

/// What ends a recording: the timer, the toggle stop file, or the hold key
/// coming back up.
enum StopWhen<'a> {
    Timer,
    ToggleFile(ToggleSession),
    KeyUp(&'a mut HoldKey),
}

pub fn run_rec_once() -> i32 {
    run_record(StopWhen::Timer)
}

pub fn run_rec_toggle() -> i32 {
    match ToggleSession::start_or_stop() {
        Ok(ToggleAction::Start(session)) => run_record(StopWhen::ToggleFile(session)),
        Ok(ToggleAction::Stop) => 0,
        Err(err) => {
            eprintln!("toggle: {err}");
            1
        }
    }
}

/// Loop forever: wait for the hold key, record while it is down, transcribe
/// and inject on release. Ctrl-C quits.
pub fn run_rec_hold() -> i32 {
    let spec = match hotkey::hold_keyspec() {
        Ok(spec) => spec,
        Err(err) => {
            eprintln!("hold: {err} (set ECHO_HOLD_KEY to a supported key)");
            return 2;
        }
    };
    let devices = match hotkey::readable_event_nodes() {
        Ok(devices) if !devices.is_empty() => devices,
        _ => {
            eprintln!("{}", hotkey::evdev_permission_hint());
            eprintln!(
                "hold mode needs readable /dev/input event devices. \
                 alternatively bind `echo-app rec --toggle` to a desktop shortcut"
            );
            return 1;
        }
    };
    let mut hold = match HoldKey::open(&devices, &spec) {
        Ok(hold) => hold,
        Err(err) => {
            eprintln!("hold: cannot open input devices: {err}");
            return 1;
        }
    };
    eprintln!("hold {} to dictate (ctrl-c to quit)", spec.keys.join("+"));
    let never = CancellationToken::new();
    loop {
        match hold.wait(HotkeyEvent::Down, &never) {
            Ok(true) => {}
            Ok(false) => return 0,
            Err(err) => {
                eprintln!("hold: {err}");
                return 1;
            }
        }
        run_record(StopWhen::KeyUp(&mut hold));
    }
}

fn run_record(mut stop: StopWhen) -> i32 {
    let mut session = Session::new();
    log_state(&session);
    let _ = status::write_status(session.state(), None);
    if matches!(HotkeySource::detect(), HotkeySource::Cli) {
        eprintln!("{}", hotkey::evdev_permission_hint());
    }
    apply_edge(&mut session, HotkeyEvent::Down);
    let _ = status::write_status(session.state(), None);
    let recording_hud = crate::ui::hud::RecordingHud::start();
    let capture = match capture_pcm(&mut stop) {
        Ok(capture) => capture,
        Err(reason) => {
            let _ = session.fail(reason);
            log_state(&session);
            let _ = status::write_status(session.state(), None);
            return 1;
        }
    };
    drop(recording_hud);
    apply_edge(&mut session, HotkeyEvent::Up);
    let _ = status::write_status(session.state(), None);

    let engine = match crate::stt::resolve_engine() {
        Some(engine) => engine,
        None => {
            let _ = session.fail(FailReason::EngineMissing);
            log_state(&session);
            let _ = status::write_status(session.state(), None);
            eprintln!(
                "no speech engine installed. install whisper-cli plus a ggml model or \
                 sherpa-onnx plus the parakeet model (see README), or set ECHO_ENGINE=fake \
                 for smoke tests"
            );
            return 1;
        }
    };
    let transcript = match engine.transcribe(&capture.pcm) {
        Ok(t) => t,
        Err(err) => {
            let reason = match err {
                echo_core::EngineError::Missing => FailReason::EngineMissing,
                echo_core::EngineError::Infer(_) => FailReason::EngineError,
            };
            let _ = session.fail(reason);
            log_state(&session);
            let _ = status::write_status(session.state(), None);
            return 1;
        }
    };

    if session.begin_cleaning().is_ok() {
        log_state(&session);
    }
    let dict = Dictionary::load().unwrap_or_else(|_| Dictionary::empty());
    let rewrite = crate::cleanup::apply(&transcript.raw, &dict);
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
    let failed = inject.failed();
    if failed {
        let reason = match &inject {
            InjectReport::Failed { reason } => *reason,
            _ => FailReason::InjectUnconfirmed,
        };
        let _ = session.fail(reason);
        log_state(&session);
    } else if session.complete_inject().is_ok() {
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
            text: rewrite.text.clone(),
            raw: transcript.raw.clone(),
            engine: transcript.engine.clone(),
            started_at,
            infer_ms: transcript.infer_ms,
            inject,
        });
    }
    if failed {
        // Leave the Failed state visible; the next session overwrites it.
        let _ = status::write_status(session.state(), None);
        return 1;
    }
    let _ = status::write_status(session.state(), Some(&rewrite.text));
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

fn capture_pcm(stop: &mut StopWhen) -> Result<audio::CaptureResult, FailReason> {
    if let Some(path) = fixture_path() {
        return audio::load_wav(&path).map_err(|_| FailReason::EngineError);
    }
    let capture = AudioCapture::open_default().map_err(|err| match err {
        audio::AudioError::NoDevice => FailReason::NoInputDevice,
        _ => FailReason::CaptureFailed,
    })?;
    let result = match stop {
        StopWhen::Timer => capture.record(recording_duration()),
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
                let result = capture.record(Duration::from_secs(MAX_RECORD_SECONDS));
                capture.cancel.cancel();
                result
            })
        }
        StopWhen::KeyUp(hold) => {
            let cancel = capture.cancel.clone();
            std::thread::scope(|scope| {
                scope.spawn(|| {
                    let _ = hold.wait(HotkeyEvent::Up, &cancel);
                    cancel.cancel();
                });
                let result = capture.record(Duration::from_secs(MAX_RECORD_SECONDS));
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
                    return Ok(ToggleAction::Start(Self {
                        lock_path,
                        stop_path,
                    }));
                }
                Err(err) if err.kind() == ErrorKind::AlreadyExists => {
                    if lock_owner_is_alive(&lock_path) {
                        fs::write(&stop_path, b"stop\n").map_err(|err| err.to_string())?;
                        return Ok(ToggleAction::Stop);
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

/// Ceiling for any recording, in seconds.
pub const MAX_RECORD_SECONDS: u64 = 60;

fn recording_duration() -> Duration {
    const DEFAULT_SECONDS: u64 = 3;
    let seconds = std::env::var("ECHO_RECORD_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .unwrap_or(DEFAULT_SECONDS)
        .min(MAX_RECORD_SECONDS);
    Duration::from_secs(seconds)
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
