# Phase 4. Linux injection

Back to [overview](./overview.md).

## Goal

A Linux `Injector` that puts text into a focused field we own. The cascade is libei or the input portal, then ydotool, then clipboard plus Ctrl+V. Clipboard contents are restored.

## Changes

`crates/echo-core/src/inject.rs` defines `Injector`, `FocusTarget`, and `InjectReport`.

`crates/echo/src/inject/linux.rs` implements the cascade. Each backend is a function, not a trait object soup.

`crates/echo/tests/inject_linux.rs` spawns a tiny GTK or winit text field, focuses it, injects a nonce, and reads the widget value back.

## Data structures

`InjectReport` is `Typed { backend } | Pasted { backend } | ClipboardOnly | Failed { reason }`. There is no `Success` boolean.

`InjectBackend` is `Libei | Ydotool | Xdotool | Wtype | ClipboardPaste`.

`FocusTarget` on Linux is `{ window_id, app_id, title }`. Missing focus is `NoFocus`, not a paste into the void.

## Verification

Static. Workspace test and clippy. Unit tests for clipboard save/restore ordering with a fake pasteboard.

Runtime. `cargo test -p echo --test inject_linux -- --ignored` must read the nonce back from the widget. A log line is not enough. Run once on X11 and once on Wayland when both are available. If libei is missing, the test still passes on a later backend, and the report names which one fired. First-run permission failures become `InjectPermission` and a printed command, not a hang.
