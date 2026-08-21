use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use echo_core::{FailReason, FocusTarget, InjectBackend, InjectReport, Injector};

use crate::which::on_path;

pub trait Pasteboard {
    fn get(&self) -> Result<String, String>;
    fn set(&self, text: &str) -> Result<(), String>;
}

#[derive(Debug, Default, Clone)]
pub struct FakePasteboard {
    inner: Arc<Mutex<String>>,
}

impl FakePasteboard {
    #[must_use]
    pub fn new(initial: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(initial.into())),
        }
    }
}

impl Pasteboard for FakePasteboard {
    fn get(&self) -> Result<String, String> {
        Ok(self.inner.lock().expect("pasteboard").clone())
    }

    fn set(&self, text: &str) -> Result<(), String> {
        *self.inner.lock().expect("pasteboard") = text.to_string();
        Ok(())
    }
}

#[derive(Debug, Default, Clone)]
pub struct SysClipboard;

impl Pasteboard for SysClipboard {
    fn get(&self) -> Result<String, String> {
        if let Ok(out) = Command::new("xclip")
            .args(["-selection", "clipboard", "-o"])
            .output()
        {
            if out.status.success() {
                return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
            }
        }
        if let Ok(out) = Command::new("wl-paste").arg("--no-newline").output() {
            if out.status.success() {
                return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
            }
        }
        Err("no clipboard tool".to_string())
    }

    fn set(&self, text: &str) -> Result<(), String> {
        if pipe_in("xclip", &["-selection", "clipboard"], text).is_ok() {
            return Ok(());
        }
        if pipe_in("wl-copy", &[], text).is_ok() {
            return Ok(());
        }
        Err("no clipboard tool".to_string())
    }
}

fn pipe_in(bin: &str, args: &[&str], text: &str) -> Result<(), String> {
    let mut child = Command::new(bin)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| err.to_string())?;
    if let Some(stdin) = child.stdin.as_mut() {
        use std::io::Write;
        stdin
            .write_all(text.as_bytes())
            .map_err(|err| err.to_string())?;
    }
    let status = child.wait().map_err(|err| err.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{bin} failed"))
    }
}

pub fn with_restored_clipboard<C: Pasteboard>(
    board: &C,
    during: impl FnOnce() -> InjectReport,
) -> InjectReport {
    let previous = board.get().ok();
    let report = during();
    if let Some(prev) = previous {
        let _ = board.set(&prev);
    }
    report
}

pub struct LinuxInjector<C> {
    clipboard: C,
}

impl Default for LinuxInjector<SysClipboard> {
    fn default() -> Self {
        Self::new()
    }
}

impl LinuxInjector<SysClipboard> {
    #[must_use]
    pub fn new() -> Self {
        Self {
            clipboard: SysClipboard,
        }
    }
}

impl<C: Pasteboard> LinuxInjector<C> {
    #[must_use]
    pub fn with_clipboard(clipboard: C) -> Self {
        Self { clipboard }
    }

    fn current_focus() -> Result<FocusTarget, FailReason> {
        if is_wayland_session() {
            return Ok(FocusTarget {
                window_id: None,
                app_id: Some("wayland".to_string()),
                title: None,
            });
        }
        let id = command_stdout("xdotool", &["getwindowfocus"]).ok();
        let title = id
            .as_deref()
            .and_then(|window| command_stdout("xdotool", &["getwindowname", window]).ok());
        let target = FocusTarget {
            window_id: id.filter(|s| !s.is_empty() && s != "0"),
            app_id: None,
            title,
        };
        if target.missing() {
            Err(FailReason::NoFocus)
        } else {
            Ok(target)
        }
    }

    fn type_text(text: &str, window: Option<&str>) -> Option<InjectBackend> {
        if is_wayland_session() && window.is_none() {
            if run_simple("ydotool", &["type", text]) {
                return Some(InjectBackend::Ydotool);
            }
            if run_simple("wtype", &[text]) {
                return Some(InjectBackend::Wtype);
            }
        }
        if let Some(id) = window {
            let _ = Command::new("xdotool")
                .args(["windowfocus", "--sync", id])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            std::thread::sleep(std::time::Duration::from_millis(30));
        }
        if run_xdotool_type(text, None, true) || run_xdotool_type(text, None, false) {
            return Some(InjectBackend::Xdotool);
        }
        if run_xdotool_type(text, window, true) || run_xdotool_type(text, window, false) {
            return Some(InjectBackend::Xdotool);
        }
        if run_simple("ydotool", &["type", text]) {
            return Some(InjectBackend::Ydotool);
        }
        if run_simple("wtype", &[text]) {
            return Some(InjectBackend::Wtype);
        }
        None
    }

    fn paste_text(&self, text: &str, window: Option<&str>) -> InjectReport {
        if self.clipboard.set(text).is_err() {
            return InjectReport::Failed {
                reason: FailReason::InjectPermission,
            };
        }
        if is_wayland_session() && window.is_none() && run_simple("ydotool", &["key", "ctrl+v"]) {
            return InjectReport::Pasted {
                backend: InjectBackend::Ydotool,
            };
        }
        if paste_key(window) {
            return InjectReport::Pasted {
                backend: InjectBackend::Xdotool,
            };
        }
        if run_simple("ydotool", &["key", "ctrl+v"]) {
            return InjectReport::Pasted {
                backend: InjectBackend::Ydotool,
            };
        }
        InjectReport::ClipboardOnly
    }
}

#[must_use]
pub fn is_wayland_session() -> bool {
    matches!(
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
        Some("wayland")
    ) || std::env::var_os("WAYLAND_DISPLAY").is_some()
}

/// Label and readiness of the injection backend a session would try first,
/// for status surfaces. Clipboard-only counts as not ready because nothing
/// lands at the cursor.
#[must_use]
pub fn detection_summary() -> (String, bool) {
    if is_wayland_session() {
        if on_path("ydotool") {
            return ("ydotool · Wayland".to_string(), true);
        }
        if on_path("wtype") {
            return ("wtype · Wayland".to_string(), true);
        }
    }
    if on_path("xdotool") {
        return ("xdotool · X11".to_string(), true);
    }
    if on_path("xclip") || on_path("wl-copy") {
        return ("Clipboard fallback".to_string(), false);
    }
    ("No injection tool found".to_string(), false)
}

impl<C: Pasteboard> Injector for LinuxInjector<C> {
    fn focus(&self) -> Result<FocusTarget, FailReason> {
        Self::current_focus()
    }

    fn inject(&self, text: &str, target: &FocusTarget) -> InjectReport {
        if target.missing() {
            return InjectReport::Failed {
                reason: FailReason::NoFocus,
            };
        }
        let window = target.window_id.as_deref();
        with_restored_clipboard(&self.clipboard, || {
            if let Some(backend) = Self::type_text(text, window) {
                return InjectReport::Typed { backend };
            }
            self.paste_text(text, window)
        })
    }
}

fn run_xdotool_type(text: &str, window: Option<&str>, clear: bool) -> bool {
    let mut cmd = Command::new("xdotool");
    cmd.arg("type");
    if clear {
        cmd.arg("--clearmodifiers");
    }
    if let Some(id) = window {
        cmd.arg("--window").arg(id);
    }
    cmd.arg("--").arg(text);
    cmd.stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn paste_key(window: Option<&str>) -> bool {
    let mut cmd = Command::new("xdotool");
    cmd.args(["key", "--clearmodifiers"]);
    if let Some(id) = window {
        cmd.arg("--window").arg(id);
    }
    cmd.arg("ctrl+v");
    cmd.stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn run_simple(bin: &str, args: &[&str]) -> bool {
    Command::new(bin)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn command_stdout(bin: &str, args: &[&str]) -> Result<String, ()> {
    let out = Command::new(bin).args(args).output().map_err(|_| ())?;
    if !out.status.success() {
        return Err(());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_save_restore_after_paste() {
        let board = FakePasteboard::new("secret");
        let report = with_restored_clipboard(&board, || {
            board.set("transcript").unwrap();
            InjectReport::Pasted {
                backend: InjectBackend::Xdotool,
            }
        });
        assert_eq!(board.get().unwrap(), "secret");
        assert_eq!(
            report,
            InjectReport::Pasted {
                backend: InjectBackend::Xdotool
            }
        );
    }

    #[test]
    fn missing_focus_is_nofocus() {
        let injector = LinuxInjector::with_clipboard(FakePasteboard::new("secret"));
        let report = injector.inject("nonce", &FocusTarget::default());
        assert_eq!(
            report,
            InjectReport::Failed {
                reason: FailReason::NoFocus
            }
        );
        assert_eq!(injector.clipboard.get().unwrap(), "secret");
    }
}
