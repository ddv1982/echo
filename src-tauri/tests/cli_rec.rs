use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../crates/echo/tests/fixtures/claude_code.wav")
}

struct ToggleHarness {
    root: PathBuf,
}

struct ManagedChild(Option<Child>);

impl ManagedChild {
    fn spawn(mut command: Command) -> Self {
        Self(Some(command.spawn().unwrap()))
    }

    fn wait_with_output(mut self) -> Output {
        let mut child = self.0.take().unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if child.try_wait().unwrap().is_some() {
                return child.wait_with_output().unwrap();
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let output = child.wait_with_output().unwrap();
                panic!(
                    "recording command did not finish; stdout={} stderr={}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn kill(mut self) {
        let mut child = self.0.take().unwrap();
        let _ = child.kill();
        let _ = child.wait();
    }
}

impl Drop for ManagedChild {
    fn drop(&mut self) {
        let Some(mut child) = self.0.take() else {
            return;
        };
        if child.try_wait().unwrap_or(None).is_none() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl ToggleHarness {
    fn new(label: &str) -> Self {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "echo-cli-toggle-{label}-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        for name in ["bin", "models", "data", "config"] {
            std::fs::create_dir_all(root.join(name)).unwrap();
        }
        std::fs::write(root.join("models/ggml-small.bin"), []).unwrap();
        let engine = root.join("bin/whisper-cli");
        std::fs::write(&engine, r#"#!/bin/sh
printf '%s\n' "$$" > "$ECHO_CONTROL_TEST_ROOT/engine-pid"
printf ready > "$ECHO_CONTROL_TEST_ROOT/engine-ready"
attempt=0
while [ ! -f "$ECHO_CONTROL_TEST_ROOT/release-engine" ]; do
  attempt=$((attempt + 1))
  [ "$attempt" -lt 1500 ] || exit 2
  /bin/sleep 0.01
done
printf '%s\n' '{"model":{"type":"small","multilingual":true},"result":{"language":"en"},"transcription":[{"text":" preserved transcript"}]}'
printf exited > "$ECHO_CONTROL_TEST_ROOT/engine-exited"
"#).unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o755)).unwrap();
        Self { root }
    }

    fn data(&self) -> PathBuf {
        self.root.join("data")
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_echo-desktop"));
        command
            .args(["rec", "--toggle"])
            .env("ECHO_CONTROL_TEST_ROOT", &self.root)
            .env("ECHO_AUDIO_FIXTURE", fixture())
            .env("ECHO_ENGINE", "whisper")
            .env("ECHO_WHISPER_MODEL", "small")
            .env("ECHO_SKIP_INJECT", "1")
            .env("ECHO_HUD", "0")
            .env("ECHO_RECORD_SECONDS", "10")
            .env("ECHO_DATA_DIR", self.data())
            .env("ECHO_CONFIG_DIR", self.root.join("config"))
            .env("ECHO_MODEL_DIR", self.root.join("models"))
            .env("PATH", self.root.join("bin"));
        command
    }

    fn start(&self) -> ManagedChild {
        let mut command = self.command();
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        ManagedChild::spawn(command)
    }

    fn toggle(&self) -> Output {
        let mut command = self.command();
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        ManagedChild::spawn(command).wait_with_output()
    }

    fn status(&self) -> String {
        std::fs::read_to_string(self.data().join("status")).unwrap_or_default()
    }

    fn wait_for(&self, label: &str, predicate: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !predicate() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            predicate(),
            "timed out waiting for {label}; status={}",
            self.status()
        );
    }

    fn wait_for_state(&self, state: &str) -> String {
        self.wait_for(state, || {
            self.status().contains(&format!("state={state}\n"))
        });
        self.status()
    }

    fn wait_for_engine(&self) {
        self.wait_for("blocked fake whisper", || {
            self.root.join("engine-ready").exists()
        });
    }

    fn release_engine(&self) {
        std::fs::write(self.root.join("release-engine"), []).unwrap();
    }

    fn wait_for_engine_exit(&self) {
        self.wait_for("fake whisper exit", || self.engine_exited());
    }

    fn engine_exited(&self) -> bool {
        if self.root.join("engine-exited").exists() {
            return true;
        }
        let Ok(pid) = std::fs::read_to_string(self.root.join("engine-pid")) else {
            return false;
        };
        let stat = std::fs::read_to_string(format!("/proc/{}/stat", pid.trim()));
        stat.map(|stat| stat.split_whitespace().nth(2) == Some("Z"))
            .unwrap_or(true)
    }

    fn reset_engine_barrier(&self) {
        let _ = std::fs::remove_file(self.root.join("engine-ready"));
        let _ = std::fs::remove_file(self.root.join("release-engine"));
        let _ = std::fs::remove_file(self.root.join("engine-exited"));
        let _ = std::fs::remove_file(self.root.join("engine-pid"));
    }
}

impl Drop for ToggleHarness {
    fn drop(&mut self) {
        let _ = std::fs::write(self.root.join("release-engine"), []);
        let deadline = Instant::now() + Duration::from_secs(5);
        while !self.engine_exited() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn session_id(status: &str) -> String {
    status
        .lines()
        .find_map(|line| line.strip_prefix("session_id="))
        .expect("status session id")
        .to_string()
}

fn history_len(data: &Path) -> usize {
    echo_core::History::load_from(data.join("history.json"))
        .expect("load history")
        .rows()
        .len()
}

fn complete_toggle_session(harness: &ToggleHarness, expected_history_len: usize) {
    let owner = harness.start();
    harness.wait_for_state("Recording");
    let stop = harness.toggle();
    assert!(
        stop.status.success(),
        "stop stderr={}",
        String::from_utf8_lossy(&stop.stderr)
    );
    harness.wait_for_engine();
    harness.release_engine();
    let output = owner.wait_with_output();
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(history_len(&harness.data()), expected_history_len);
    let history =
        echo_core::History::load_from(harness.data().join("history.json")).expect("load history");
    assert_eq!(history.rows().last().unwrap().text, "preserved transcript");
}

#[test]
fn rec_once_drives_recording_then_transcribing() {
    let data = std::env::temp_dir().join(format!("echo-rec-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&data);
    let bin = env!("CARGO_BIN_EXE_echo-desktop");
    let out = Command::new(bin)
        .args(["rec", "--once"])
        .env("ECHO_AUDIO_FIXTURE", fixture())
        .env("ECHO_ENGINE", "fake")
        .env("ECHO_SKIP_INJECT", "1")
        .env("ECHO_DATA_DIR", &data)
        .output()
        .expect("run echo-desktop rec --once");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("session Recording"),
        "stdout={stdout:?} stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("session Transcribing"), "stdout={stdout:?}");
}

#[test]
fn rec_once_uses_the_global_recording_lease() {
    let root = std::env::temp_dir().join(format!("echo-rec-lease-{}", std::process::id()));
    let data = root.join("data");
    let config = root.join("config");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&config).unwrap();
    let bin = env!("CARGO_BIN_EXE_echo-desktop");
    let command = || {
        let mut command = Command::new(bin);
        command
            .args(["rec", "--once"])
            .env("ECHO_AUDIO_FIXTURE", fixture())
            .env("ECHO_ENGINE", "fake")
            .env("ECHO_SKIP_INJECT", "1")
            .env("ECHO_DATA_DIR", &data)
            .env("ECHO_CONFIG_DIR", &config);
        command
    };
    let mut first = command().spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !data.join("recording.lock").exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(data.join("recording.lock").exists());

    let second = command().output().unwrap();

    assert_eq!(second.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&second.stderr).contains("Another recording"));
    assert!(first.wait().unwrap().success());
}

#[test]
fn rec_once_uses_config_file_engine_when_env_unset() {
    let root = std::env::temp_dir().join(format!("echo-config-engine-{}", std::process::id()));
    let config_dir = root.join("config");
    let data = root.join("data");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(config_dir.join("config.json"), r#"{"engine":"fake"}"#).unwrap();
    let bin = env!("CARGO_BIN_EXE_echo-desktop");
    let out = Command::new(bin)
        .args(["rec", "--once"])
        .env("ECHO_AUDIO_FIXTURE", fixture())
        .env("ECHO_SKIP_INJECT", "1")
        .env("ECHO_DATA_DIR", &data)
        .env("ECHO_CONFIG_DIR", &config_dir)
        .env_remove("ECHO_ENGINE")
        .output()
        .expect("run echo-desktop rec --once");
    assert!(
        out.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let history = echo_core::History::load_from(data.join("history.json")).unwrap();
    let text = history
        .rows()
        .iter()
        .map(|row| row.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        text.to_ascii_lowercase().contains("claude code"),
        "history={text:?}"
    );
}

#[test]
fn rec_once_rejects_corrupt_config_before_capture() {
    let root = std::env::temp_dir().join(format!("echo-corrupt-config-{}", std::process::id()));
    let config_dir = root.join("config");
    let data = root.join("data");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(config_dir.join("config.json"), "not json").unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_echo-desktop"))
        .args(["rec", "--once"])
        .env("ECHO_AUDIO_FIXTURE", fixture())
        .env("ECHO_DATA_DIR", &data)
        .env("ECHO_CONFIG_DIR", &config_dir)
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("invalid JSON"));
    assert!(!data.join("history.json").exists());
}

#[test]
fn rec_once_without_engine_fails_engine_missing() {
    let data = std::env::temp_dir().join(format!("echo-rec-noengine-{}", std::process::id()));
    let models = data.join("models");
    let config_dir = data.join("config");
    let _ = std::fs::create_dir_all(&models);
    let _ = std::fs::create_dir_all(&config_dir);
    let bin = env!("CARGO_BIN_EXE_echo-desktop");
    let out = Command::new(bin)
        .args(["rec", "--once"])
        .env("ECHO_AUDIO_FIXTURE", fixture())
        .env("ECHO_MODEL_DIR", &models)
        .env("ECHO_DATA_DIR", &data)
        .env("ECHO_CONFIG_DIR", &config_dir)
        .env_remove("ECHO_ENGINE")
        .output()
        .expect("run echo-desktop rec --once");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "expected failure, stdout={stdout:?}");
    assert!(
        stdout.contains("session Failed speech engine or model missing"),
        "stdout={stdout:?} stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn rec_once_missing_microphone_name_still_records() {
    let root = std::env::temp_dir().join(format!("echo-config-mic-{}", std::process::id()));
    let config_dir = root.join("config");
    let data = root.join("data");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(
        config_dir.join("config.json"),
        r#"{"engine":"fake","microphone":"no-such-device"}"#,
    )
    .unwrap();
    let bin = env!("CARGO_BIN_EXE_echo-desktop");
    let out = Command::new(bin)
        .args(["rec", "--once"])
        .env("ECHO_AUDIO_FIXTURE", fixture())
        .env("ECHO_SKIP_INJECT", "1")
        .env("ECHO_DATA_DIR", &data)
        .env("ECHO_CONFIG_DIR", &config_dir)
        .env_remove("ECHO_ENGINE")
        .output()
        .expect("run echo-desktop rec --once");
    assert!(
        out.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn rec_once_without_mic_names_permission() {
    let bin = env!("CARGO_BIN_EXE_echo-desktop");
    let mut cmd = Command::new(bin);
    cmd.args(["rec", "--once"]);
    cmd.env_remove("ECHO_AUDIO_FIXTURE");
    cmd.env("ECHO_RECORD_SECONDS", "1");
    let out = cmd.output().expect("run echo-desktop rec --once");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        text.contains("session Recording"),
        "expected a Recording edge, got {text:?}"
    );
    if !text.contains("session Transcribing") {
        assert!(
            text.contains("microphone"),
            "expected a permission reason, got {text:?}"
        );
    }
}

#[test]
fn rec_toggle_stops_saves_cancels_and_restarts() {
    let harness = ToggleHarness::new("lifecycle");

    complete_toggle_session(&harness, 1);

    harness.reset_engine_barrier();
    let cancelled = harness.start();
    harness.wait_for_state("Recording");
    assert!(harness.toggle().status.success());
    harness.wait_for_engine();
    assert!(harness
        .wait_for_state("Transcribing")
        .contains("state=Transcribing\n"));
    assert!(!std::fs::read_dir(harness.data()).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("recording.cancel")
    }));
    assert!(harness.toggle().status.success());
    let output = cancelled.wait_with_output();
    assert!(
        !output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(history_len(&harness.data()), 1);

    harness.reset_engine_barrier();
    complete_toggle_session(&harness, 2);
}

#[derive(Clone, Copy)]
enum OwnerDeathBoundary {
    Capture,
    Transcription,
}

fn replacement_ignores_dead_owner_intents(boundary: OwnerDeathBoundary) {
    let harness = ToggleHarness::new(match boundary {
        OwnerDeathBoundary::Capture => "capture-death",
        OwnerDeathBoundary::Transcription => "transcription-death",
    });
    let dead_owner = harness.start();
    let old_status = harness.wait_for_state("Recording");
    let old_token = session_id(&old_status);

    if matches!(boundary, OwnerDeathBoundary::Transcription) {
        assert!(harness.toggle().status.success());
        harness.wait_for_engine();
        harness.wait_for_state("Transcribing");
    }
    dead_owner.kill();
    if matches!(boundary, OwnerDeathBoundary::Transcription) {
        harness.release_engine();
        harness.wait_for_engine_exit();
        harness.reset_engine_barrier();
    }

    let replacement = harness.start();
    harness.wait_for("replacement recording", || {
        let status = harness.status();
        status.contains("state=Recording\n") && session_id(&status) != old_token
    });
    let replacement_status = harness.status();
    let replacement_token = session_id(&replacement_status);
    assert_ne!(replacement_token, old_token);
    let lock = std::fs::read_to_string(harness.data().join("recording.lock")).unwrap();
    assert!(lock.contains(&replacement_token));
    assert!(!lock.contains(&old_token));

    std::fs::write(
        harness.data().join("recording.stop"),
        format!("{old_token}\n"),
    )
    .unwrap();
    std::fs::write(
        harness.data().join("recording.cancel"),
        format!("{old_token}\n"),
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(75));
    let after_stale_intents = harness.wait_for_state("Recording");
    assert_eq!(session_id(&after_stale_intents), replacement_token);

    assert!(harness.toggle().status.success());
    harness.wait_for_engine();
    harness.release_engine();
    let output = replacement.wait_with_output();
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(history_len(&harness.data()), 1);
}

#[test]
fn rec_toggle_recovers_after_owner_death_during_capture() {
    replacement_ignores_dead_owner_intents(OwnerDeathBoundary::Capture);
}

#[test]
fn rec_toggle_recovers_after_owner_death_during_blocked_transcription() {
    replacement_ignores_dead_owner_intents(OwnerDeathBoundary::Transcription);
}
