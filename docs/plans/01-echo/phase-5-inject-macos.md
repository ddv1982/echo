# Phase 5. macOS injection

Back to [overview](./overview.md).

## Goal

The same `Injector` trait, a Mac cascade that survives Electron. AX insert only when the focused element is a real text field. Otherwise clipboard plus Cmd+V. Restore the pasteboard. Never treat AX success as proof in Chromium.

## Changes

`crates/echo/src/inject/macos.rs` talks to `AXUIElement` and `CGEvent`. Keep the unsafe bits in this file.

`crates/echo/src/inject/macos_pasteboard.rs` saves and restores `NSPasteboard`.

`crates/echo/tests/inject_macos.rs` is `#[cfg(target_os = "macos")]`. It drives TextEdit through Accessibility for the happy path. A second ignored test asks an operator to focus Cursor and checks that the report is `Pasted`, not `Typed`.

## Data structures

Reuse `InjectReport`. Add `InjectBackend::AxSetValue` and `InjectBackend::CgEventPaste`.

`AxFocus` is `TextField { role } | Missing | NonText { role }`. `Missing` still attempts Cmd+V. That is the Chrome `kAXErrorNoValue` case. `NonText` does not paste.

## Verification

Static. The Linux CI still compiles the crate. macOS modules are `cfg`-gated so clippy on this host stays green.

Runtime. This phase cannot be proven on the Cloud Agent. On a Mac, `cargo test -p echo --test inject_macos -- --ignored` must show the nonce in TextEdit. The Cursor case must land in the buffer, not only on the clipboard. If it only hits the clipboard, the phase is not done. That is the bug the video spent a turn on.
