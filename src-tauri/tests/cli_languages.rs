use std::path::Path;
use std::process::Command;

fn run(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_echo-desktop"))
        .args(args)
        .env("ECHO_CONFIG_DIR", root.join("config"))
        .env("ECHO_DATA_DIR", root.join("data"))
        .env("ECHO_MODEL_DIR", root.join("models"))
        .env_remove("ECHO_ENGINE")
        .env_remove("ECHO_WHISPER_MODEL")
        .env_remove("ECHO_LANGUAGE")
        .output()
        .unwrap()
}

#[test]
fn languages_is_model_aware_and_never_advertises_fake() {
    let root = std::env::temp_dir().join(format!("echo-cli-languages-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("models")).unwrap();

    let multilingual = run(
        &root,
        &["languages", "--engine", "whisper", "--format", "json"],
    );
    assert!(multilingual.status.success());
    let json: serde_json::Value = serde_json::from_slice(&multilingual.stdout).unwrap();
    assert_eq!(json["schemaVersion"], 1);
    assert_eq!(json["engine"], "whisper");
    assert_eq!(json["selection"], "auto-or-pinned");
    assert_eq!(json["languages"].as_array().unwrap().len(), 100);

    std::fs::write(root.join("models/ggml-base.en.bin"), []).unwrap();
    let english = run(
        &root,
        &["languages", "--engine", "whisper", "--format", "json"],
    );
    let json: serde_json::Value = serde_json::from_slice(&english.stdout).unwrap();
    assert_eq!(json["model"], "base.en");
    assert_eq!(json["selection"], "english-only");
    assert_eq!(json["languages"].as_array().unwrap().len(), 1);
    assert_eq!(json["languages"][0]["code"], "en");

    let parakeet = run(
        &root,
        &["languages", "--engine", "parakeet", "--format", "json"],
    );
    let json: serde_json::Value = serde_json::from_slice(&parakeet.stdout).unwrap();
    assert_eq!(json["engine"], "parakeet");
    assert_eq!(json["selection"], "automatic-only");
    assert_eq!(json["languages"].as_array().unwrap().len(), 25);

    let text = run(&root, &["languages", "--engine", "parakeet"]);
    assert!(text.status.success());
    let text = String::from_utf8(text.stdout).unwrap();
    assert!(text.starts_with("engine\tparakeet\nmodel\ttdt-0.6b-v3\n"));
    assert!(text.contains("selection\tautomatic-only\n"));
    assert!(!text.contains("fake"));
    assert!(text.ends_with('\n'));
}
