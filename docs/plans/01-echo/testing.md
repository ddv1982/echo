# Testing

Back to [overview](./overview.md).

There is no control skill for a native overlay. Proof is a mix of `cargo test`, ignored live tests, and a short manual checklist. A green unit suite is not the product. Every item below runs on Linux.

## Static, every phase

```
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

## Runtime harness

`crates/echo/tests/compare_engines.rs` is the video's comparison window in test form. One WAV fixture. Every enabled engine. Print `engine`, `infer_ms`, and `raw`. Do not scrape another app's local database for timings.

`crates/echo/tests/inject_linux.rs` reads a nonce back from a widget we spawn.

`crates/echo/tests/record_once.rs` is the live mic check.

## Manual checklist

1. Grant mic access. Speak while `echo rec --once` runs. History shows text.
2. Focus a terminal. Hold the hotkey. The nonce or the spoken sentence appears in that terminal.
3. Repeat in Firefox or Chromium, in a text box.
4. On Wayland, confirm `InjectReport` names `Ydotool` or `Wtype`, not a fake X11 success.
5. Copy a secret, dictate, confirm the clipboard holds the secret again.
6. Hide the main window. Dictation still works from the tray process.
7. Dictionary entry for a known mishear. HUD or history shows Corrected.
