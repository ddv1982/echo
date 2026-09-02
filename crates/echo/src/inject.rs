use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use echo_core::{FailReason, FocusTarget, InjectBackend, InjectReport, Injector};

use crate::which::on_path;

pub trait Pasteboard {
    fn set(&self, text: &str) -> Result<(), String>;
}

trait CommandRunner: Send + Sync {
    fn run(&self, bin: &str, args: &[&str]) -> bool;
}

#[derive(Debug, Default)]
struct SystemCommandRunner;

impl CommandRunner for SystemCommandRunner {
    fn run(&self, bin: &str, args: &[&str]) -> bool {
        run_simple(bin, args)
    }
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

    #[must_use]
    pub fn text(&self) -> String {
        self.inner.lock().expect("pasteboard").clone()
    }
}

impl Pasteboard for FakePasteboard {
    fn set(&self, text: &str) -> Result<(), String> {
        *self.inner.lock().expect("pasteboard") = text.to_string();
        Ok(())
    }
}

#[derive(Debug, Default, Clone)]
pub struct SysClipboard;

impl Pasteboard for SysClipboard {
    fn set(&self, text: &str) -> Result<(), String> {
        let commands: [(&str, &[&str]); 2] = if is_wayland_session() {
            [("wl-copy", &[]), ("xclip", &["-selection", "clipboard"])]
        } else {
            [("xclip", &["-selection", "clipboard"]), ("wl-copy", &[])]
        };
        for (bin, args) in commands {
            if pipe_in(bin, args, text).is_ok() {
                return Ok(());
            }
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

pub struct LinuxInjector<C> {
    clipboard: C,
    runner: Arc<dyn CommandRunner>,
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
            runner: Arc::new(SystemCommandRunner),
        }
    }
}

impl<C: Pasteboard> LinuxInjector<C> {
    #[must_use]
    pub fn with_clipboard(clipboard: C) -> Self {
        Self {
            clipboard,
            runner: Arc::new(SystemCommandRunner),
        }
    }

    #[cfg(test)]
    fn with_runner(clipboard: C, runner: Arc<dyn CommandRunner>) -> Self {
        Self { clipboard, runner }
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

    fn type_text(&self, text: &str, window: Option<&str>) -> Option<InjectBackend> {
        if let Some(id) = window {
            let _ = self.runner.run("xdotool", &["windowfocus", "--sync", id]);
            std::thread::sleep(std::time::Duration::from_millis(30));
            return (run_xdotool_type(self.runner.as_ref(), text, Some(id), true)
                || run_xdotool_type(self.runner.as_ref(), text, Some(id), false))
            .then_some(InjectBackend::Xdotool);
        }

        if is_wayland_session() && window.is_none() {
            if self.runner.run("ydotool", &["type", text]) {
                return Some(InjectBackend::Ydotool);
            }
            if self.runner.run("wtype", &[text]) {
                return Some(InjectBackend::Wtype);
            }
        }
        if run_xdotool_type(self.runner.as_ref(), text, None, true)
            || run_xdotool_type(self.runner.as_ref(), text, None, false)
        {
            return Some(InjectBackend::Xdotool);
        }
        if run_xdotool_type(self.runner.as_ref(), text, window, true)
            || run_xdotool_type(self.runner.as_ref(), text, window, false)
        {
            return Some(InjectBackend::Xdotool);
        }
        if self.runner.run("ydotool", &["type", text]) {
            return Some(InjectBackend::Ydotool);
        }
        if self.runner.run("wtype", &[text]) {
            return Some(InjectBackend::Wtype);
        }
        None
    }

    fn paste_text(&self, text: &str, window: Option<&str>) -> InjectReport {
        self.paste_with(text, || {
            if window.is_some() {
                return paste_key(self.runner.as_ref(), window).then_some(InjectBackend::Xdotool);
            }
            if is_wayland_session() && self.runner.run("ydotool", &["key", "ctrl+v"]) {
                return Some(InjectBackend::Ydotool);
            }
            if paste_key(self.runner.as_ref(), window) {
                return Some(InjectBackend::Xdotool);
            }
            self.runner
                .run("ydotool", &["key", "ctrl+v"])
                .then_some(InjectBackend::Ydotool)
        })
    }

    fn paste_with(
        &self,
        text: &str,
        dispatch: impl FnOnce() -> Option<InjectBackend>,
    ) -> InjectReport {
        if self.clipboard.set(text).is_err() {
            return InjectReport::Failed {
                reason: FailReason::InjectPermission,
            };
        }
        dispatch()
            .map(|backend| InjectReport::Pasted { backend })
            .unwrap_or(InjectReport::ClipboardOnly)
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
        if let Some(backend) = self.type_text(text, window) {
            return InjectReport::Typed { backend };
        }
        self.paste_text(text, window)
    }
}

fn run_xdotool_type(
    runner: &dyn CommandRunner,
    text: &str,
    window: Option<&str>,
    clear: bool,
) -> bool {
    let mut args = vec!["type"];
    if clear {
        args.push("--clearmodifiers");
    }
    if let Some(id) = window {
        args.extend(["--window", id]);
    }
    args.extend(["--", text]);
    runner.run("xdotool", &args)
}

fn paste_key(runner: &dyn CommandRunner, window: Option<&str>) -> bool {
    let mut args = vec!["key", "--clearmodifiers"];
    if let Some(id) = window {
        args.extend(["--window", id]);
    }
    args.push("ctrl+v");
    runner.run("xdotool", &args)
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

    type Call = (String, Vec<String>);

    #[derive(Clone)]
    struct RecordingRunner {
        calls: Arc<Mutex<Vec<Call>>>,
        reject_targeted: bool,
    }

    impl RecordingRunner {
        fn new(reject_targeted: bool) -> Self {
            Self {
                calls: Arc::new(Mutex::new(Vec::new())),
                reject_targeted,
            }
        }

        fn calls(&self) -> Vec<Call> {
            self.calls.lock().expect("runner calls").clone()
        }
    }

    impl CommandRunner for RecordingRunner {
        fn run(&self, bin: &str, args: &[&str]) -> bool {
            self.calls.lock().expect("runner calls").push((
                bin.to_string(),
                args.iter().map(|arg| (*arg).to_string()).collect(),
            ));
            !self.reject_targeted || !args.contains(&"--window")
        }
    }

    fn captured_target() -> FocusTarget {
        FocusTarget {
            window_id: Some("4242".to_string()),
            app_id: None,
            title: Some("captured".to_string()),
        }
    }

    fn assert_only_targeted_x11_dispatch(calls: &[Call]) {
        let dispatches: Vec<_> = calls
            .iter()
            .filter(|(bin, args)| {
                bin == "xdotool" && matches!(args.first().map(String::as_str), Some("type" | "key"))
            })
            .collect();
        assert!(!dispatches.is_empty(), "no X11 dispatch was attempted");
        for (_, args) in dispatches {
            assert!(
                args.windows(2)
                    .any(|pair| pair[0] == "--window" && pair[1] == "4242"),
                "untargeted X11 dispatch: {args:?}"
            );
        }
        assert!(
            calls
                .iter()
                .all(|(bin, _)| bin != "ydotool" && bin != "wtype"),
            "global fallback was attempted: {calls:?}"
        );
    }

    #[test]
    fn clipboard_only_keeps_transcript_available() {
        let board = FakePasteboard::new("secret");
        let injector = LinuxInjector::with_clipboard(board.clone());
        let report = injector.paste_with("transcript", || None);
        assert_eq!(board.text(), "transcript");
        assert_eq!(report, InjectReport::ClipboardOnly);
    }

    #[test]
    fn pasted_text_stays_available_after_key_dispatch() {
        let board = FakePasteboard::new("secret");
        let injector = LinuxInjector::with_clipboard(board.clone());
        let report = injector.paste_with("transcript", || Some(InjectBackend::Xdotool));
        assert_eq!(board.text(), "transcript");
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
        assert_eq!(injector.clipboard.text(), "secret");
    }

    #[test]
    fn captured_x11_typing_uses_only_the_captured_window() {
        let runner = RecordingRunner::new(false);
        let injector =
            LinuxInjector::with_runner(FakePasteboard::new("secret"), Arc::new(runner.clone()));

        let report = injector.inject("nonce", &captured_target());

        assert_eq!(
            report,
            InjectReport::Typed {
                backend: InjectBackend::Xdotool
            }
        );
        assert_only_targeted_x11_dispatch(&runner.calls());
    }

    #[test]
    fn captured_x11_target_failure_does_not_use_global_fallback() {
        let runner = RecordingRunner::new(true);
        let board = FakePasteboard::new("secret");
        let injector = LinuxInjector::with_runner(board.clone(), Arc::new(runner.clone()));

        let report = injector.inject("nonce", &captured_target());

        assert_eq!(report, InjectReport::ClipboardOnly);
        assert_eq!(board.text(), "nonce");
        let calls = runner.calls();
        assert_only_targeted_x11_dispatch(&calls);
        assert!(
            calls
                .iter()
                .any(|(bin, args)| bin == "xdotool"
                    && args.first().map(String::as_str) == Some("key")),
            "targeted paste was not attempted: {calls:?}"
        );
    }
}
