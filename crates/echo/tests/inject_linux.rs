use echo::inject::{LinuxInjector, SysClipboard};
use echo_core::{FocusTarget, InjectReport, Injector};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

fn widget_bin() -> Result<PathBuf, String> {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/x11_widget.c");
    let out = std::env::temp_dir().join("echo-x11-widget");
    let status = Command::new("cc")
        .args([
            "-o",
            out.to_str().ok_or("widget path")?,
            src.to_str().ok_or("widget source")?,
            "-lX11",
        ])
        .status()
        .map_err(|err| err.to_string())?;
    if !status.success() {
        return Err("cc -lX11 failed".to_string());
    }
    Ok(out)
}

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
fn inject_nonce_into_owned_widget() {
    if !display_ok() {
        eprintln!("skip: DISPLAY is not usable");
        return;
    }
    let bin = match widget_bin() {
        Ok(path) => path,
        Err(err) => {
            eprintln!("skip: could not create widget ({err})");
            return;
        }
    };
    let nonce = format!("echo{}", std::process::id());
    let out = std::env::temp_dir().join(format!("echo-inject-{}.txt", std::process::id()));
    let ready = std::env::temp_dir().join(format!("echo-inject-{}-ready", std::process::id()));
    let _ = fs::remove_file(&out);
    let _ = fs::remove_file(&ready);
    let mut child = Command::new(&bin)
        .arg(&out)
        .arg(&ready)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn widget");
    let deadline = Instant::now() + Duration::from_secs(3);
    while !ready.exists() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(20));
    }
    let window = fs::read_to_string(&ready).expect("widget ready file");
    let window = window.trim().to_string();
    assert!(!window.is_empty(), "widget did not publish a window id");
    let injector = LinuxInjector::<SysClipboard>::new();
    let target = FocusTarget {
        window_id: Some(window.clone()),
        app_id: None,
        title: Some("echo-inject-target".to_string()),
    };
    let report = injector.inject(&nonce, &target);
    match report {
        InjectReport::Typed { .. } | InjectReport::Pasted { .. } => {}
        other => panic!("expected typed or pasted, got {other:?}"),
    }
    let finished = Instant::now() + Duration::from_secs(2);
    let mut got = String::new();
    while Instant::now() < finished {
        got = fs::read_to_string(&out).unwrap_or_default();
        if got.contains(&nonce) {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    let _ = Command::new("xdotool").args(["key", "Return"]).status();
    let _ = child.wait();
    assert!(
        got.contains(&nonce),
        "widget read back {got:?}, report={report:?}"
    );
}
