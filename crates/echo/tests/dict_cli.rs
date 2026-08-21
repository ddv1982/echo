use std::process::Command;

#[test]
fn dict_add_round_trip() {
    let dir = std::env::temp_dir().join(format!("echo-dict-cli-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bin = env!("CARGO_BIN_EXE_echo");
    let out = Command::new(bin)
        .args(["dict", "add", "clawed code", "Claude Code"])
        .env("ECHO_DATA_DIR", &dir)
        .output()
        .expect("dict add");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let store = echo_core::Dictionary::load_from(dir.join("dictionary.json")).unwrap();
    let rewrite = store.rewrite("open clawed code");
    assert_eq!(rewrite.text, "open Claude Code");
}
