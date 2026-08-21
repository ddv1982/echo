use std::process::Command;

fn readable_event_nodes_exist() -> bool {
    let Ok(entries) = std::fs::read_dir("/dev/input") else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("event"))
            && std::fs::File::open(&path).is_ok()
    })
}

#[test]
fn rec_hold_without_evdev_fails_with_hint() {
    if readable_event_nodes_exist() {
        eprintln!("skipping: readable evdev devices exist here; hold mode would block on a key");
        return;
    }
    let bin = env!("CARGO_BIN_EXE_echo-app");
    let out = Command::new(bin)
        .args(["rec", "--hold"])
        .output()
        .expect("run echo-app rec --hold");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("hold mode needs readable /dev/input"),
        "stderr={stderr:?}"
    );
}
