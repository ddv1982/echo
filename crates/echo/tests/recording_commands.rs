use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

struct Owner(Child);

impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn command(root: &Path, helper: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", helper, "--ignored", "--nocapture"])
        .env("ECHO_CONTROL_TEST_ROOT", root)
        .env("ECHO_DATA_DIR", root.join("data"))
        .env("ECHO_CONFIG_DIR", root.join("config"))
        .env("ECHO_MODEL_DIR", root.join("models"))
        .env("ECHO_ENGINE", "whisper")
        .env("ECHO_WHISPER_MODEL", "small")
        .env("ECHO_SKIP_INJECT", "1")
        .env("ECHO_HUD", "0")
        .env("ECHO_RECORD_SECONDS", "10")
        .env(
            "ECHO_AUDIO_FIXTURE",
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude_code.wav"),
        )
        .env("PATH", root.join("bin"))
        .stdout(Stdio::null());
    command
}

fn wait_for(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !path.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(path.exists(), "missing {}", path.display());
}

fn wait(child: &mut Child) -> ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("recording command did not finish");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn control(root: &Path, action: &str, session: &str, accepted: bool) {
    let mut child = command(root, "control_helper")
        .env("ECHO_CONTROL_TEST_ACTION", action)
        .env("ECHO_CONTROL_TEST_SESSION", session)
        .env("ECHO_CONTROL_TEST_ACCEPTED", accepted.to_string())
        .spawn()
        .unwrap();
    assert!(wait(&mut child).success());
}

fn exercise_transcription(cancel: bool, legacy_stop: bool) {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    for name in ["bin", "models", "data", "config"] {
        std::fs::create_dir(root.path().join(name)).unwrap();
    }
    std::fs::write(root.path().join("models/ggml-small.bin"), []).unwrap();
    let runtime = root.path().join("bin/whisper-cli");
    std::fs::write(
        &runtime,
        r#"#!/bin/sh
printf ready > "$ECHO_CONTROL_TEST_ROOT/engine-ready"
attempt=0
while [ ! -f "$ECHO_CONTROL_TEST_ROOT/release-engine" ]; do
  attempt=$((attempt + 1))
  [ "$attempt" -lt 1000 ] || exit 2
  /bin/sleep 0.01
done
printf '%s\n' '{"model":{"type":"small","multilingual":true},"result":{"language":"en"},"transcription":[{"text":" preserved transcript"}]}'
"#,
    )
    .unwrap();
    std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o755)).unwrap();

    let mut owner = Owner(command(root.path(), "owner_helper").spawn().unwrap());
    let token_path = root.path().join("session");
    wait_for(&token_path);
    let session = std::fs::read_to_string(&token_path).unwrap();
    control(root.path(), "stop", "replaced-session", false);
    if legacy_stop {
        std::fs::write(
            root.path().join("data/recording.stop"),
            format!("{session}\n"),
        )
        .unwrap();
    } else {
        control(root.path(), "stop", &session, true);
    }
    wait_for(&root.path().join("engine-ready"));
    control(root.path(), "stop", &session, false);
    let status = std::fs::read_to_string(root.path().join("data/status")).unwrap();
    assert!(status
        .lines()
        .any(|line| line == format!("pid={}", owner.0.id())));
    assert!(status.contains("state=Transcribing\n"));
    assert!(!std::fs::read_dir(root.path().join("data"))
        .unwrap()
        .any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("recording.cancel")));
    assert!(!root.path().join("data/history.json").exists());

    if cancel {
        control(root.path(), "cancel", "replaced-session", false);
        control(root.path(), "cancel", &session, true);
    } else {
        std::fs::write(root.path().join("release-engine"), []).unwrap();
    }
    assert!(wait(&mut owner.0).success());
    if cancel {
        assert!(!root.path().join("data/history.json").exists());
        assert!(std::fs::read_to_string(root.path().join("data/status"))
            .unwrap()
            .starts_with("state=Failed"));
    } else {
        let history = echo_core::History::load_from(root.path().join("data/history.json")).unwrap();
        assert_eq!(history.rows().len(), 1);
        assert_eq!(history.rows()[0].text, "preserved transcript");
    }
}

#[test]
fn duplicate_capture_stop_preserves_the_running_transcription() {
    exercise_transcription(false, false);
}

#[test]
fn explicit_cancel_terminates_the_running_transcription() {
    exercise_transcription(true, false);
}

#[test]
fn legacy_flat_capture_stop_preserves_transcription() {
    exercise_transcription(false, true);
}

#[test]
#[ignore = "isolated recording owner"]
fn owner_helper() {
    let Some(root) = std::env::var_os("ECHO_CONTROL_TEST_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let session = echo::rec::start_managed_recording().unwrap();
    let status = echo::status::read();
    assert_eq!(
        status.session_id.as_deref(),
        Some(session.session_id.as_str())
    );
    assert!(session.revision > 0 && status.revision >= session.revision);
    echo_core::write_atomic_private(&root.join("session"), session.session_id.as_bytes()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while echo::rec::session_active() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!echo::rec::session_active());
}

#[test]
#[ignore = "isolated recording requester"]
fn control_helper() {
    let Ok(action) = std::env::var("ECHO_CONTROL_TEST_ACTION") else {
        return;
    };
    let session = std::env::var("ECHO_CONTROL_TEST_SESSION").unwrap();
    let accepted = match action.as_str() {
        "stop" => echo::rec::request_capture_stop(&session),
        "cancel" => echo::rec::request_transcription_cancel(&session),
        _ => panic!("unknown test command"),
    }
    .unwrap();
    assert_eq!(
        accepted,
        std::env::var("ECHO_CONTROL_TEST_ACCEPTED").unwrap() == "true"
    );
}
