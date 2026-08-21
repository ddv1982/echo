use echo::cleanup::LocalCleanup;
use echo_core::{Cleanup, Dictionary};

#[test]
#[ignore = "needs a cleanup binary on PATH"]
fn local_model_removes_fillers() {
    let bin = std::env::var("ECHO_CLEANUP_BIN").unwrap_or_else(|_| "echo-cleanup".to_string());
    let which = std::process::Command::new("which")
        .arg(&bin)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    assert!(which, "set ECHO_CLEANUP_BIN to a stdin/stdout cleaner");
    let rewrite = LocalCleanup { bin }
        .apply("um so like can we uh move the button", &Dictionary::empty())
        .expect("local cleanup");
    let hay = rewrite.text.to_ascii_lowercase();
    assert!(!hay.split_whitespace().any(|w| w == "um" || w == "uh"));
}
