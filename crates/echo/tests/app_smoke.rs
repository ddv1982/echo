use std::process::{Command, Stdio};

fn display_ok() -> bool {
    std::env::var_os("DISPLAY").is_some()
        && Command::new("xdpyinfo")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
}

#[test]
fn app_smoke_inits_and_exits() {
    if !display_ok() {
        return;
    }
    let bin = env!("CARGO_BIN_EXE_echo-app");
    let out = Command::new(bin)
        .env("ECHO_APP_SMOKE", "1")
        .output()
        .expect("run echo-app smoke");
    assert!(
        out.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn quit_after_flag_inits_and_exits() {
    if !display_ok() {
        return;
    }
    let bin = env!("CARGO_BIN_EXE_echo-app");
    let out = Command::new(bin)
        .arg("--quit-after=0")
        .output()
        .expect("run echo-app --quit-after=0");
    assert!(
        out.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
}
