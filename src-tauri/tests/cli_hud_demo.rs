use std::process::Command;

#[test]
fn hud_demo_starts_and_exits() {
    let bin = env!("CARGO_BIN_EXE_echo-desktop");
    let out = Command::new(bin)
        .arg("--hud-demo")
        .output()
        .expect("run echo-desktop --hud-demo");
    assert!(
        out.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
}
