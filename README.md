# Echo

Local hold-to-talk dictation for Linux. You hold a key, speak, and cleaned text lands at the cursor. Audio never leaves the machine.

The first-build plan is [docs/plans/01-echo/overview.md](docs/plans/01-echo/overview.md).

## Build

You need Rust 1.83, a C compiler, ALSA headers for `cpal`, and `xdotool` on X11.

```
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo build --release
```

The default tests do not open a microphone, do not download models, and do not need the network after crates are fetched.

## Run

```
echo rec --once
echo dict add "Claude Code"
echo dict add "clawed code" "Claude Code"
echo history
echo status
echo --hud-demo
```

`echo rec --once` moves the session from Idle to Recording to Transcribing. Set `ECHO_AUDIO_FIXTURE` to a 16 kHz WAV if you have no mic. Bind a compositor key to `echo rec --once` when evdev is not readable.

Models live under `$XDG_CACHE_HOME/echo` or `ECHO_MODEL_DIR`. Echo looks for `whisper-cli` and `sherpa-onnx-offline` on `PATH`. Missing files return `EngineMissing`. It does not download the Parakeet tarball.

Dictionary and history live under `$XDG_DATA_HOME/echo`, or `$HOME/.local/share/echo`. Tests override that with `ECHO_DATA_DIR`.

Cleanup defaults to rules mode. It drops standalone um, uh, and like, then capitalizes and adds ending punctuation. Set `ECHO_CLEANUP=off` to skip that pass. `ECHO_CLEANUP=local:binary` runs a stdin/stdout program on `PATH`.

## Status file

There is no tray icon. A tray crate would pull a desktop toolkit that this binary does not otherwise need. `echo status` reads `$XDG_DATA_HOME/echo/status`.

## Inject

On X11 the cascade is `xdotool type`, then clipboard plus Ctrl+V, then restore the clipboard. Wayland wants libei, `ydotool`, or `wtype` when those tools exist. A log line that says the insert worked is not enough. `cargo test -p echo --test inject_linux` types a nonce into a widget this repo compiles and reads that nonce back.

## Live checks

These stay ignored until you have hardware or cached models.

```
ECHO_LIVE_MIC=1 cargo test -p echo --test record_once -- --ignored
cargo test -p echo --test transcribe_fixture -- --ignored
cargo test -p echo --test compare_engines -- --ignored
```
