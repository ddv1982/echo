use std::sync::atomic::{AtomicBool, Ordering};

use echo_core::FailReason;

/// Set by the bare CLI entry points (`rec` subcommands run in `try_cli`
/// before the Tauri builder). The GUI's in-process sessions leave it unset:
/// their failures already show in the window through the status poll.
static NOTIFY_FAILURES: AtomicBool = AtomicBool::new(false);

pub fn enable_failure_notifications() {
    NOTIFY_FAILURES.store(true, Ordering::Relaxed);
}

#[must_use]
pub fn failure_notifications_enabled() -> bool {
    NOTIFY_FAILURES.load(Ordering::Relaxed)
}

/// One sentence per failure, naming the fix. The audience pressed a
/// compositor shortcut in another app; the journal is not where they look.
#[must_use]
pub fn failure_message(reason: FailReason, detail: Option<&str>) -> String {
    match reason {
        FailReason::NoInputDevice => {
            "Echo couldn't record: no microphone input device. \
             Open Echo → Settings to pick one."
                .to_string()
        }
        FailReason::CaptureFailed => {
            "Echo couldn't record: microphone capture failed. \
             Open Echo → Settings and test the microphone."
                .to_string()
        }
        FailReason::EngineMissing => {
            "Echo couldn't transcribe: no speech engine or model installed. \
             Open Echo → Settings to download one."
                .to_string()
        }
        FailReason::EngineError => {
            let detail = detail
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .unwrap_or("the speech engine failed");
            format!("Echo couldn't transcribe: {detail}. Open Echo → Settings for the last-run details.")
        }
        FailReason::InjectPermission => {
            "Echo transcribed but couldn't insert: text insertion permission denied. \
             Open Echo → Settings for the insertion setup."
                .to_string()
        }
        FailReason::NoFocus => {
            "Echo transcribed but found no focused window to type into. \
             Click where the text should go and dictate again."
                .to_string()
        }
        FailReason::InjectUnconfirmed => {
            "Echo transcribed but the insert was not confirmed. \
             Open Echo → Settings for the insertion setup."
                .to_string()
        }
    }
}

/// Tell the user a shortcut-spawned session failed, on the session bus.
/// A failed notification logs and is swallowed: it must never fail or change
/// the session it reports on.
pub fn notify_session_failure(reason: FailReason, detail: Option<&str>) {
    if !failure_notifications_enabled() {
        return;
    }
    let message = failure_message(reason, detail);
    let result = notify_rust::Notification::new()
        .appname("Echo")
        .summary("Echo")
        .body(&message)
        .icon("echo-desktop")
        .show();
    if let Err(err) = result {
        eprintln!("notification failed: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_failure_reason_has_a_sentence_with_a_fix() {
        for reason in [
            FailReason::NoInputDevice,
            FailReason::CaptureFailed,
            FailReason::InjectPermission,
            FailReason::EngineMissing,
            FailReason::NoFocus,
            FailReason::EngineError,
            FailReason::InjectUnconfirmed,
        ] {
            let message = failure_message(reason, None);
            assert!(message.contains("Echo"), "{reason:?}: {message}");
            assert!(message.contains("Settings") || message.contains("dictate again"), "{reason:?}: {message}");
        }
        let detailed = failure_message(FailReason::EngineError, Some("ggml_init failed"));
        assert!(detailed.contains("ggml_init failed"));
    }

    #[test]
    fn notifications_are_off_unless_enabled() {
        assert!(!failure_notifications_enabled());
        enable_failure_notifications();
        assert!(failure_notifications_enabled());
    }
}
