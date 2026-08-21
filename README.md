# Echo

Local hold-to-talk dictation for Linux. You hold a key, speak, and cleaned text lands at the cursor. Audio never leaves the machine.

The first-build plan is [docs/plans/01-echo/overview.md](docs/plans/01-echo/overview.md).

## Build

You need Rust 1.83, a C compiler, ALSA headers for `cpal`, GTK 3, Ayatana AppIndicator, and `xdotool` on X11.

```
sudo apt install libgtk-3-dev libayatana-appindicator3-dev
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo build --release
```

The default tests do not open a microphone, do not download models, and do not need the network after crates are fetched.

## Run the tray app

The binary is `echo-app`. Cargo also links the same file as `echo` for tests. The shell already has a builtin named `echo`, so `echo rec --once` will not run this program.

```
cargo run -p echo --
./target/release/echo-app
```

A tray icon appears. The menu is Record, History, Dictionary, and Quit. Close a window to hide it. The process stays running until you choose Quit.

`ECHO_APP_SMOKE=1` or `--quit-after=0` initializes GTK, loads the icon, and exits 0. Use that for a display check that should not hang.

## CLI

```
echo-app rec --once
echo-app dict add "Claude Code"
echo-app dict add "clawed code" "Claude Code"
echo-app history
echo-app status
echo-app --hud-demo
```

`echo-app rec --once` moves the session from Idle to Recording to Transcribing. Set `ECHO_AUDIO_FIXTURE` to a 16 kHz WAV if you have no mic. Bind a compositor key to `echo-app rec --once` when evdev is not readable.

Models live under `$XDG_CACHE_HOME/echo` or `ECHO_MODEL_DIR`. Echo looks for `whisper-cli` and `sherpa-onnx-offline` on `PATH`. Missing files return `EngineMissing`. It does not download the Parakeet tarball.

Dictionary and history live under `$XDG_DATA_HOME/echo`, or `$HOME/.local/share/echo`. Tests override that with `ECHO_DATA_DIR`.

Cleanup defaults to rules mode. It drops standalone um, uh, and like, then capitalizes and adds ending punctuation. Set `ECHO_CLEANUP=off` to skip that pass. `ECHO_CLEANUP=local:binary` runs a stdin/stdout program on `PATH`.

## Status file

The tray writes `$XDG_DATA_HOME/echo/status` as the session moves. `echo-app status` reads that file. The tray tooltip uses the same state line.

## Install the desktop entry

```
mkdir -p ~/.local/share/applications
mkdir -p ~/.local/share/icons/hicolor/scalable/apps
mkdir -p ~/.local/share/icons/hicolor/256x256/apps
cp packaging/echo.desktop ~/.local/share/applications/
cp assets/icons/echo.svg ~/.local/share/icons/hicolor/scalable/apps/echo.svg
ffmpeg -y -i assets/icons/echo.png -vf scale=256:256 ~/.local/share/icons/hicolor/256x256/apps/echo.png
update-desktop-database ~/.local/share/applications
gtk-update-icon-cache ~/.local/share/icons/hicolor
```

`packaging/echo.desktop` runs `echo-app` and uses `Icon=echo`. Put `echo-app` on `PATH`, for example `/usr/local/bin/echo-app`. Leave `assets/icons/echo.png` in the repo as the 1024 source. The tray embeds that PNG, so the icon still works before you install a theme copy.

## Inject

On X11 the cascade is `xdotool type`, then clipboard plus Ctrl+V, then restore the clipboard. Wayland wants libei, `ydotool`, or `wtype` when those tools exist. A log line that says the insert worked is not enough. `cargo test -p echo --test inject_linux` types a nonce into a widget this repo compiles and reads that nonce back.

## Live checks

These stay ignored until you have hardware or cached models.

```
ECHO_LIVE_MIC=1 cargo test -p echo --test record_once -- --ignored
cargo test -p echo --test transcribe_fixture -- --ignored
cargo test -p echo --test compare_engines -- --ignored
```
