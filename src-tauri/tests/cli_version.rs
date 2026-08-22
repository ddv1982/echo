use std::process::Command;

#[test]
fn version_flag_prints_the_package_version() {
    let bin = env!("CARGO_BIN_EXE_echo-desktop");
    for flag in ["--version", "-V"] {
        let out = Command::new(bin)
            .arg(flag)
            .output()
            .expect("run echo-desktop --version");
        assert!(out.status.success(), "{flag} exited {}", out.status);
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert_eq!(
            stdout.trim(),
            format!("echo-desktop {}", env!("CARGO_PKG_VERSION")),
            "{flag} output"
        );
    }
}
