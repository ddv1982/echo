# Testing

Back to [overview](./overview.md).

There is no control skill for a native overlay on Linux or macOS. Proof is a mix of `cargo test`, ignored live tests, and a short manual checklist. A green unit suite is not the product.

## Static, every phase

```
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

Run on this Linux host. macOS-only modules stay behind `cfg` so the command stays green here.

## Runtime harness

`crates/echo/tests/compare_engines.rs` is the video's comparison window in test form. One WAV fixture. Every enabled engine. Print `engine`, `infer_ms`, and `raw`. Do not scrape Wispr Flow's local database. That number in the video includes IPC they do not own.

`crates/echo/tests/inject_linux.rs` reads a nonce back from a widget we spawn.

`crates/echo/tests/inject_macos.rs` does the same to TextEdit, then Cursor, on a Mac.

`crates/echo/tests/record_once.rs` is the live mic check.

## Manual checklist, Linux

1. Grant mic access. Speak while `echo rec --once` runs. History shows text.
2. Focus a terminal. Hold the hotkey. The nonce or the spoken sentence appears in that terminal.
3. Repeat in Firefox or Chromium, in a text box.
4. On Wayland, confirm `InjectReport` names `Libei` or `Ydotool`, not a fake X11 success.
5. Copy a secret, dictate, confirm the clipboard holds the secret again.

## Manual checklist, macOS

1. Grant Microphone and Accessibility. Input Monitoring if the tap asks.
2. TextEdit insert works.
3. Cursor insert works. If the report says `Typed` and the buffer is empty, you have the video's AX bug. Fail the phase.
4. Hide the main window. Dictation still works from the tray process.
5. Dictionary entry for a known mishear. HUD or history shows Corrected.

## What we will not pretend

This Cloud Agent cannot launch a Mac GUI. Phases 5, 6 (macOS tap), 7 (focus theft), and 9 (hide window) stay unproven until a Mac run. Write that in the implementation PR, do not hide it behind "tested locally."
