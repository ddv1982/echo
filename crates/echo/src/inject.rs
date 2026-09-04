use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use echo_core::{FailReason, FocusTarget, InjectBackend, InjectReport, Injector};

use crate::hotkey::DesktopSession;
use crate::which::on_path;

pub trait Pasteboard {
    fn get(&self) -> Result<String, String> {
        Err("clipboard read unsupported".to_string())
    }

    fn set(&self, text: &str) -> Result<(), String>;

    /// Put `text` on the clipboard, submit the paste command, and report
    /// whether a clipboard data transfer was actually observed. The default
    /// is deliberately conservative because most clipboard implementations
    /// cannot observe which data requests a submitted key command caused.
    fn set_and_dispatch(
        &self,
        text: &str,
        dispatch: &mut dyn FnMut() -> Option<InjectBackend>,
    ) -> Result<(Option<InjectBackend>, bool), String> {
        self.set(text)?;
        Ok((dispatch(), false))
    }

    /// Restore a text snapshot only while Echo's transcript is still current.
    /// This text comparison cannot preserve non-text clipboard formats.
    fn restore_if_unchanged(&self, expected: &str, previous: &str) -> Result<bool, String> {
        if self.get()? != expected {
            return Ok(false);
        }
        self.set(previous)?;
        Ok(true)
    }
}

trait CommandRunner: Send + Sync {
    fn run(&self, bin: &str, args: &[&str], stdin: Option<&str>) -> bool;
}

#[derive(Debug, Default)]
struct SystemCommandRunner;

impl CommandRunner for SystemCommandRunner {
    fn run(&self, bin: &str, args: &[&str], stdin: Option<&str>) -> bool {
        match stdin {
            Some(text) => pipe_in(bin, args, text).is_ok(),
            None => run_simple(bin, args),
        }
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
    fn get(&self) -> Result<String, String> {
        Ok(self.text())
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
        let commands: [(&str, &[&str]); 2] = match DesktopSession::current() {
            DesktopSession::Wayland => [
                ("wl-paste", &["--no-newline"]),
                ("xclip", &["-selection", "clipboard", "-o"]),
            ],
            DesktopSession::X11 | DesktopSession::Unknown => [
                ("xclip", &["-selection", "clipboard", "-o"]),
                ("wl-paste", &["--no-newline"]),
            ],
        };
        for (bin, args) in commands {
            if let Ok(text) = command_stdout_exact(bin, args) {
                return Ok(text);
            }
        }
        Err("no clipboard tool".to_string())
    }

    fn set(&self, text: &str) -> Result<(), String> {
        let commands: [(&str, &[&str]); 2] = match DesktopSession::current() {
            DesktopSession::Wayland => [("wl-copy", &[]), ("xclip", &["-selection", "clipboard"])],
            DesktopSession::X11 | DesktopSession::Unknown => {
                [("xclip", &["-selection", "clipboard"]), ("wl-copy", &[])]
            }
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
    let write_result = child.stdin.take().map_or_else(
        || Err(format!("{bin} stdin unavailable")),
        |mut stdin| {
            use std::io::Write;
            stdin
                .write_all(text.as_bytes())
                .map_err(|err| err.to_string())
        },
    );
    if let Err(error) = write_result {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
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
    session: DesktopSession,
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
            session: DesktopSession::current(),
        }
    }
}

impl<C: Pasteboard> LinuxInjector<C> {
    #[must_use]
    pub fn with_clipboard(clipboard: C) -> Self {
        Self {
            clipboard,
            runner: Arc::new(SystemCommandRunner),
            session: DesktopSession::current(),
        }
    }

    #[cfg(test)]
    fn with_runner(clipboard: C, runner: Arc<dyn CommandRunner>, session: DesktopSession) -> Self {
        Self {
            clipboard,
            runner,
            session,
        }
    }

    fn current_focus(&self) -> Result<FocusTarget, FailReason> {
        if self.session == DesktopSession::Wayland {
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
            let _ = self
                .runner
                .run("xdotool", &["windowfocus", "--sync", id], None);
            std::thread::sleep(std::time::Duration::from_millis(30));
            return run_xdotool_type(self.runner.as_ref(), text, Some(id))
                .then_some(InjectBackend::Xdotool);
        }

        match self.session {
            DesktopSession::Wayland => {
                if self
                    .runner
                    .run("ydotool", &["type", "--file", "-"], Some(text))
                {
                    return Some(InjectBackend::Ydotool);
                }
                if self.runner.run("wtype", &["--", text], None) {
                    return Some(InjectBackend::Wtype);
                }
                run_xdotool_type(self.runner.as_ref(), text, None).then_some(InjectBackend::Xdotool)
            }
            DesktopSession::X11 | DesktopSession::Unknown => {
                if run_xdotool_type(self.runner.as_ref(), text, None) {
                    return Some(InjectBackend::Xdotool);
                }
                if self
                    .runner
                    .run("ydotool", &["type", "--file", "-"], Some(text))
                {
                    return Some(InjectBackend::Ydotool);
                }
                self.runner
                    .run("wtype", &["--", text], None)
                    .then_some(InjectBackend::Wtype)
            }
        }
    }

    fn paste_text(&self, text: &str, window: Option<&str>) -> InjectReport {
        let report = self.paste_with(text, || {
            if window.is_some() {
                return paste_key(self.runner.as_ref(), window).then_some(InjectBackend::Xdotool);
            }
            match self.session {
                DesktopSession::Wayland => {
                    if self.runner.run("ydotool", &["key", "ctrl+v"], None) {
                        return Some(InjectBackend::Ydotool);
                    }
                    paste_key(self.runner.as_ref(), None).then_some(InjectBackend::Xdotool)
                }
                DesktopSession::X11 | DesktopSession::Unknown => {
                    if paste_key(self.runner.as_ref(), None) {
                        return Some(InjectBackend::Xdotool);
                    }
                    self.runner
                        .run("ydotool", &["key", "ctrl+v"], None)
                        .then_some(InjectBackend::Ydotool)
                }
            }
        });
        match (window, report) {
            (Some(_), InjectReport::ClipboardOnly) => InjectReport::Failed {
                reason: FailReason::InjectUnconfirmed,
            },
            (_, report) => report,
        }
    }

    fn paste_with(
        &self,
        text: &str,
        dispatch: impl FnOnce() -> Option<InjectBackend>,
    ) -> InjectReport {
        let previous = self.clipboard.get().ok();
        let mut dispatch = Some(dispatch);
        let result = self.clipboard.set_and_dispatch(text, &mut || {
            dispatch.take().and_then(|dispatch| dispatch())
        });
        let Ok((backend, transferred)) = result else {
            return InjectReport::Failed {
                reason: FailReason::InjectPermission,
            };
        };
        let Some(backend) = backend else {
            return InjectReport::ClipboardOnly;
        };
        if !transferred {
            return InjectReport::ClipboardOnly;
        }
        if let Some(previous) = previous {
            if let Err(error) = self.clipboard.restore_if_unchanged(text, &previous) {
                eprintln!("clipboard restore failed: {error}");
            }
        }
        InjectReport::Pasted { backend }
    }
}

#[must_use]
pub fn is_wayland_session() -> bool {
    DesktopSession::current() == DesktopSession::Wayland
}

/// Label and readiness of the injection backend a session would try first,
/// for status surfaces. Clipboard-only counts as not ready because nothing
/// lands at the cursor.
#[must_use]
pub fn detection_summary() -> (String, bool) {
    if DesktopSession::current() == DesktopSession::Wayland {
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
        self.current_focus()
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

fn run_xdotool_type(runner: &dyn CommandRunner, text: &str, window: Option<&str>) -> bool {
    let mut args = vec!["type", "--clearmodifiers"];
    if let Some(id) = window {
        args.extend(["--window", id]);
    }
    args.extend(["--", text]);
    runner.run("xdotool", &args, None)
}

fn paste_key(runner: &dyn CommandRunner, window: Option<&str>) -> bool {
    let mut args = vec!["key", "--clearmodifiers"];
    if let Some(id) = window {
        args.extend(["--window", id]);
    }
    args.push("ctrl+v");
    runner.run("xdotool", &args, None)
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

fn command_stdout_exact(bin: &str, args: &[&str]) -> Result<String, String> {
    let out = Command::new(bin)
        .args(args)
        .stderr(Stdio::null())
        .output()
        .map_err(|error| error.to_string())?;
    if !out.status.success() {
        return Err(format!("{bin} failed"));
    }
    String::from_utf8(out.stdout).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    type Call = (String, Vec<String>, Option<String>);

    #[derive(Clone)]
    struct RecordingRunner {
        calls: Arc<Mutex<Vec<Call>>>,
        outcomes: Arc<Mutex<VecDeque<bool>>>,
    }

    impl RecordingRunner {
        fn new(outcomes: impl IntoIterator<Item = bool>) -> Self {
            Self {
                calls: Arc::new(Mutex::new(Vec::new())),
                outcomes: Arc::new(Mutex::new(outcomes.into_iter().collect())),
            }
        }

        fn calls(&self) -> Vec<Call> {
            self.calls.lock().expect("runner calls").clone()
        }
    }

    impl CommandRunner for RecordingRunner {
        fn run(&self, bin: &str, args: &[&str], stdin: Option<&str>) -> bool {
            self.calls.lock().expect("runner calls").push((
                bin.to_string(),
                args.iter().map(|arg| (*arg).to_string()).collect(),
                stdin.map(str::to_string),
            ));
            self.outcomes
                .lock()
                .expect("runner outcomes")
                .pop_front()
                .unwrap_or(false)
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum ClipboardOp {
        Get,
        Set(String),
    }

    #[derive(Clone)]
    struct RecordingPasteboard {
        state: Arc<Mutex<ClipboardState>>,
    }

    struct ClipboardState {
        text: String,
        ops: Vec<ClipboardOp>,
        get_error: Option<String>,
        get_error_on_call: usize,
        get_count: usize,
        set_results: VecDeque<Result<(), String>>,
        transfer_confirmed: bool,
        concurrent_after_dispatch: Option<String>,
    }

    impl RecordingPasteboard {
        fn new(initial: &str) -> Self {
            Self::configured(initial, None, [])
        }

        fn configured(
            initial: &str,
            get_error: Option<&str>,
            set_results: impl IntoIterator<Item = Result<(), String>>,
        ) -> Self {
            Self {
                state: Arc::new(Mutex::new(ClipboardState {
                    text: initial.to_string(),
                    ops: Vec::new(),
                    get_error: get_error.map(str::to_string),
                    get_error_on_call: 1,
                    get_count: 0,
                    set_results: set_results.into_iter().collect(),
                    transfer_confirmed: false,
                    concurrent_after_dispatch: None,
                })),
            }
        }

        fn confirmed(self) -> Self {
            self.state
                .lock()
                .expect("clipboard state")
                .transfer_confirmed = true;
            self
        }

        fn changed_during_dispatch(self, text: &str) -> Self {
            self.state
                .lock()
                .expect("clipboard state")
                .concurrent_after_dispatch = Some(text.to_string());
            self
        }

        fn fail_restore_check(self, error: &str) -> Self {
            let mut state = self.state.lock().expect("clipboard state");
            state.get_error = Some(error.to_string());
            state.get_error_on_call = 2;
            drop(state);
            self
        }

        fn text(&self) -> String {
            self.state.lock().expect("clipboard state").text.clone()
        }

        fn ops(&self) -> Vec<ClipboardOp> {
            self.state.lock().expect("clipboard state").ops.clone()
        }
    }

    impl Pasteboard for RecordingPasteboard {
        fn get(&self) -> Result<String, String> {
            let mut state = self.state.lock().expect("clipboard state");
            state.ops.push(ClipboardOp::Get);
            state.get_count += 1;
            if state.get_count == state.get_error_on_call {
                if let Some(error) = &state.get_error {
                    return Err(error.clone());
                }
            }
            Ok(state.text.clone())
        }

        fn set(&self, text: &str) -> Result<(), String> {
            let mut state = self.state.lock().expect("clipboard state");
            state.ops.push(ClipboardOp::Set(text.to_string()));
            let result = state.set_results.pop_front().unwrap_or(Ok(()));
            if result.is_ok() {
                state.text = text.to_string();
            }
            result
        }

        fn set_and_dispatch(
            &self,
            text: &str,
            dispatch: &mut dyn FnMut() -> Option<InjectBackend>,
        ) -> Result<(Option<InjectBackend>, bool), String> {
            self.set(text)?;
            let backend = dispatch();
            let mut state = self.state.lock().expect("clipboard state");
            if let Some(concurrent) = state.concurrent_after_dispatch.take() {
                state.text = concurrent;
            }
            Ok((backend, state.transfer_confirmed && backend.is_some()))
        }
    }

    fn call(bin: &str, args: &[&str]) -> Call {
        (
            bin.to_string(),
            args.iter().map(|arg| (*arg).to_string()).collect(),
            None,
        )
    }

    fn call_with_stdin(bin: &str, args: &[&str], stdin: &str) -> Call {
        (
            bin.to_string(),
            args.iter().map(|arg| (*arg).to_string()).collect(),
            Some(stdin.to_string()),
        )
    }

    fn injector(
        board: RecordingPasteboard,
        runner: &RecordingRunner,
        session: DesktopSession,
    ) -> LinuxInjector<RecordingPasteboard> {
        LinuxInjector::with_runner(board, Arc::new(runner.clone()), session)
    }

    fn captured_target() -> FocusTarget {
        FocusTarget {
            window_id: Some("4242".to_string()),
            app_id: None,
            title: Some("captured".to_string()),
        }
    }

    fn untargeted_target() -> FocusTarget {
        FocusTarget {
            window_id: None,
            app_id: Some("application".to_string()),
            title: None,
        }
    }

    #[test]
    fn fake_pasteboard_get_returns_exact_text() {
        let board = FakePasteboard::new("exact\ntext");
        assert_eq!(board.get(), Ok("exact\ntext".to_string()));
    }

    #[test]
    fn pasteboard_get_default_preserves_set_only_implementors() {
        struct SetOnly;

        impl Pasteboard for SetOnly {
            fn set(&self, _text: &str) -> Result<(), String> {
                Ok(())
            }
        }

        assert_eq!(SetOnly.get(), Err("clipboard read unsupported".to_string()));
    }

    #[test]
    fn wayland_typing_tries_each_backend_once_with_exact_ydotool_stdin() {
        let runner = RecordingRunner::new([false, false, false]);
        let board = RecordingPasteboard::new("secret");
        let injector = injector(board.clone(), &runner, DesktopSession::Wayland);
        let text = "-leading\nsecond line";

        assert_eq!(injector.type_text(text, None), None);
        assert_eq!(
            runner.calls(),
            vec![
                call_with_stdin("ydotool", &["type", "--file", "-"], text),
                call("wtype", &["--", text]),
                call("xdotool", &["type", "--clearmodifiers", "--", text]),
            ]
        );
        assert!(board.ops().is_empty());
    }

    #[test]
    fn x11_typing_tries_each_backend_once_in_order() {
        let runner = RecordingRunner::new([false, false, false]);
        let injector = injector(
            RecordingPasteboard::new("secret"),
            &runner,
            DesktopSession::X11,
        );

        assert_eq!(injector.type_text("-leading", None), None);
        assert_eq!(
            runner.calls(),
            vec![
                call("xdotool", &["type", "--clearmodifiers", "--", "-leading"]),
                call_with_stdin("ydotool", &["type", "--file", "-"], "-leading"),
                call("wtype", &["--", "-leading"]),
            ]
        );
    }

    #[test]
    fn unknown_typing_uses_x11_order() {
        let runner = RecordingRunner::new([false, false, false]);
        let injector = injector(
            RecordingPasteboard::new("secret"),
            &runner,
            DesktopSession::Unknown,
        );

        assert_eq!(injector.type_text("text", None), None);
        assert_eq!(
            runner.calls(),
            vec![
                call("xdotool", &["type", "--clearmodifiers", "--", "text"]),
                call_with_stdin("ydotool", &["type", "--file", "-"], "text"),
                call("wtype", &["--", "text"]),
            ]
        );
    }

    #[test]
    fn typing_stops_after_each_possible_successful_backend() {
        let cases = [
            (
                DesktopSession::Wayland,
                vec![true],
                InjectBackend::Ydotool,
                1,
            ),
            (
                DesktopSession::Wayland,
                vec![false, true],
                InjectBackend::Wtype,
                2,
            ),
            (
                DesktopSession::Wayland,
                vec![false, false, true],
                InjectBackend::Xdotool,
                3,
            ),
        ];
        for (session, outcomes, expected, call_count) in cases {
            let runner = RecordingRunner::new(outcomes);
            let injector = injector(RecordingPasteboard::new("secret"), &runner, session);
            assert_eq!(injector.type_text("text", None), Some(expected));
            assert_eq!(runner.calls().len(), call_count);
        }
    }

    #[test]
    fn captured_x11_typing_focuses_and_types_once_without_clipboard() {
        let runner = RecordingRunner::new([true, true]);
        let board = RecordingPasteboard::new("secret");
        let injector = injector(board.clone(), &runner, DesktopSession::X11);

        let report = injector.inject("-nonce", &captured_target());

        assert_eq!(
            report,
            InjectReport::Typed {
                backend: InjectBackend::Xdotool
            }
        );
        assert_eq!(
            runner.calls(),
            vec![
                call("xdotool", &["windowfocus", "--sync", "4242"]),
                call(
                    "xdotool",
                    &[
                        "type",
                        "--clearmodifiers",
                        "--window",
                        "4242",
                        "--",
                        "-nonce"
                    ]
                ),
            ]
        );
        assert!(board.ops().is_empty());
    }

    #[test]
    fn untargeted_typed_path_does_not_touch_clipboard() {
        let runner = RecordingRunner::new([true]);
        let board = RecordingPasteboard::new("secret");
        let injector = injector(board.clone(), &runner, DesktopSession::Wayland);

        let report = injector.inject("nonce", &untargeted_target());

        assert_eq!(
            report,
            InjectReport::Typed {
                backend: InjectBackend::Ydotool
            }
        );
        assert!(board.ops().is_empty());
    }

    #[test]
    fn missing_focus_does_not_touch_commands_or_clipboard() {
        let runner = RecordingRunner::new([]);
        let board = RecordingPasteboard::new("secret");
        let injector = injector(board.clone(), &runner, DesktopSession::X11);

        let report = injector.inject("nonce", &FocusTarget::default());

        assert_eq!(
            report,
            InjectReport::Failed {
                reason: FailReason::NoFocus
            }
        );
        assert!(runner.calls().is_empty());
        assert!(board.ops().is_empty());
        assert_eq!(board.text(), "secret");
    }

    #[test]
    fn wayland_paste_dispatches_ydotool_then_xdotool_once() {
        let runner = RecordingRunner::new([false, true]);
        let board = RecordingPasteboard::new("old").confirmed();
        let injector = injector(board.clone(), &runner, DesktopSession::Wayland);

        let report = injector.paste_text("transcript", None);

        assert_eq!(
            report,
            InjectReport::Pasted {
                backend: InjectBackend::Xdotool
            }
        );
        assert_eq!(
            runner.calls(),
            vec![
                call("ydotool", &["key", "ctrl+v"]),
                call("xdotool", &["key", "--clearmodifiers", "ctrl+v"]),
            ]
        );
        assert_eq!(
            board.ops(),
            vec![
                ClipboardOp::Get,
                ClipboardOp::Set("transcript".to_string()),
                ClipboardOp::Get,
                ClipboardOp::Set("old".to_string()),
            ]
        );
        assert_eq!(board.text(), "old");
    }

    #[test]
    fn x11_and_unknown_paste_dispatch_xdotool_then_ydotool_once() {
        for session in [DesktopSession::X11, DesktopSession::Unknown] {
            let runner = RecordingRunner::new([false, true]);
            let board = RecordingPasteboard::new("old").confirmed();
            let injector = injector(board.clone(), &runner, session);

            assert_eq!(
                injector.paste_text("transcript", None),
                InjectReport::Pasted {
                    backend: InjectBackend::Ydotool
                }
            );
            assert_eq!(
                runner.calls(),
                vec![
                    call("xdotool", &["key", "--clearmodifiers", "ctrl+v"]),
                    call("ydotool", &["key", "ctrl+v"]),
                ]
            );
            assert_eq!(board.text(), "old");
        }
    }

    #[test]
    fn initial_clipboard_set_failure_does_not_dispatch() {
        let runner = RecordingRunner::new([true]);
        let board = RecordingPasteboard::configured("old", None, [Err("set denied".to_string())]);
        let injector = injector(board.clone(), &runner, DesktopSession::X11);

        let report = injector.paste_text("transcript", None);

        assert_eq!(
            report,
            InjectReport::Failed {
                reason: FailReason::InjectPermission
            }
        );
        assert!(runner.calls().is_empty());
        assert_eq!(
            board.ops(),
            vec![ClipboardOp::Get, ClipboardOp::Set("transcript".to_string())]
        );
        assert_eq!(board.text(), "old");
    }

    #[test]
    fn submitted_but_unconfirmed_untargeted_paste_leaves_transcript() {
        let runner = RecordingRunner::new([true]);
        let board = RecordingPasteboard::new("old");
        let injector = injector(board.clone(), &runner, DesktopSession::X11);

        assert_eq!(
            injector.paste_text("transcript", None),
            InjectReport::ClipboardOnly
        );
        assert_eq!(board.text(), "transcript");
        assert_eq!(
            board.ops(),
            vec![ClipboardOp::Get, ClipboardOp::Set("transcript".to_string())]
        );
        assert_eq!(
            runner.calls(),
            vec![call("xdotool", &["key", "--clearmodifiers", "ctrl+v"])]
        );
    }

    #[test]
    fn submitted_but_unconfirmed_targeted_paste_reports_failure() {
        let runner = RecordingRunner::new([true, false, true]);
        let board = RecordingPasteboard::new("old");
        let injector = injector(board.clone(), &runner, DesktopSession::X11);

        let report = injector.inject("nonce", &captured_target());

        assert_eq!(
            report,
            InjectReport::Failed {
                reason: FailReason::InjectUnconfirmed
            }
        );
        assert_eq!(
            runner.calls(),
            vec![
                call("xdotool", &["windowfocus", "--sync", "4242"]),
                call(
                    "xdotool",
                    &[
                        "type",
                        "--clearmodifiers",
                        "--window",
                        "4242",
                        "--",
                        "nonce"
                    ]
                ),
                call(
                    "xdotool",
                    &["key", "--clearmodifiers", "--window", "4242", "ctrl+v"]
                ),
            ]
        );
        assert_eq!(board.text(), "nonce");
        assert_eq!(
            board.ops(),
            vec![ClipboardOp::Get, ClipboardOp::Set("nonce".to_string())]
        );
    }

    #[test]
    fn successful_targeted_paste_restores_previous_clipboard() {
        let runner = RecordingRunner::new([true, false, true]);
        let board = RecordingPasteboard::new("old").confirmed();
        let injector = injector(board.clone(), &runner, DesktopSession::X11);

        assert_eq!(
            injector.inject("nonce", &captured_target()),
            InjectReport::Pasted {
                backend: InjectBackend::Xdotool
            }
        );
        assert_eq!(board.text(), "old");
        assert_eq!(runner.calls().len(), 3);
        assert_eq!(
            board.ops(),
            vec![
                ClipboardOp::Get,
                ClipboardOp::Set("nonce".to_string()),
                ClipboardOp::Get,
                ClipboardOp::Set("old".to_string()),
            ]
        );
    }

    #[test]
    fn clipboard_get_failure_skips_restore_after_successful_paste() {
        let runner = RecordingRunner::new([true]);
        let board = RecordingPasteboard::configured("old", Some("read denied"), []).confirmed();
        let injector = injector(board.clone(), &runner, DesktopSession::X11);

        assert_eq!(
            injector.paste_text("transcript", None),
            InjectReport::Pasted {
                backend: InjectBackend::Xdotool
            }
        );
        assert_eq!(board.text(), "transcript");
        assert_eq!(
            board.ops(),
            vec![ClipboardOp::Get, ClipboardOp::Set("transcript".to_string())]
        );
    }

    #[test]
    fn clipboard_restore_failure_remains_pasted_without_repeat() {
        let runner = RecordingRunner::new([true]);
        let board = RecordingPasteboard::configured(
            "old",
            None,
            [Ok(()), Err("restore denied".to_string())],
        )
        .confirmed();
        let injector = injector(board.clone(), &runner, DesktopSession::X11);

        assert_eq!(
            injector.paste_text("transcript", None),
            InjectReport::Pasted {
                backend: InjectBackend::Xdotool
            }
        );
        assert_eq!(runner.calls().len(), 1);
        assert_eq!(board.text(), "transcript");
        assert_eq!(
            board.ops(),
            vec![
                ClipboardOp::Get,
                ClipboardOp::Set("transcript".to_string()),
                ClipboardOp::Get,
                ClipboardOp::Set("old".to_string()),
            ]
        );
    }

    #[test]
    fn clipboard_restore_guard_read_failure_does_not_restore() {
        let runner = RecordingRunner::new([true]);
        let board = RecordingPasteboard::new("old")
            .confirmed()
            .fail_restore_check("read denied");
        let injector = injector(board.clone(), &runner, DesktopSession::X11);

        assert_eq!(
            injector.paste_text("transcript", None),
            InjectReport::Pasted {
                backend: InjectBackend::Xdotool
            }
        );
        assert_eq!(board.text(), "transcript");
        assert_eq!(
            board.ops(),
            vec![
                ClipboardOp::Get,
                ClipboardOp::Set("transcript".to_string()),
                ClipboardOp::Get,
            ]
        );
    }

    #[test]
    fn confirmed_transfer_does_not_overwrite_concurrent_clipboard_change() {
        let runner = RecordingRunner::new([true]);
        let board = RecordingPasteboard::new("old")
            .confirmed()
            .changed_during_dispatch("newer");
        let injector = injector(board.clone(), &runner, DesktopSession::X11);

        assert_eq!(
            injector.paste_text("transcript", None),
            InjectReport::Pasted {
                backend: InjectBackend::Xdotool
            }
        );
        assert_eq!(board.text(), "newer");
        assert_eq!(
            board.ops(),
            vec![
                ClipboardOp::Get,
                ClipboardOp::Set("transcript".to_string()),
                ClipboardOp::Get,
            ]
        );
    }
}
