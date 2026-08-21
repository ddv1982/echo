# Echo

Local dictation for Linux. Hold a key (or press a toggle shortcut), speak, and cleaned text lands at the cursor. Audio never leaves the machine.

Hold-to-talk (`echo-desktop rec --hold`) reads the key from `/dev/input` and needs the user in the `input` group. The toggle (`echo-desktop rec --toggle`) works everywhere through a desktop keyboard shortcut.

The first-build plan is [docs/plans/01-echo/overview.md](docs/plans/01-echo/overview.md).

## Download

Tagged alpha builds are on [GitHub Releases](https://github.com/ddv1982/echo/releases). Nightly Linux artifacts come from the [release workflow](https://github.com/ddv1982/echo/actions/workflows/release.yml).

## Build

You need Rust 1.88 or newer and Node.js 22 or newer. On Ubuntu, Debian, Zorin OS, and their derivatives, install the native build and runtime dependencies with:

```sh
sudo apt update
sudo apt install build-essential pkg-config libasound2-dev \
  libwebkit2gtk-4.1-dev libdbus-1-dev libayatana-appindicator3-dev \
  xdotool
```

Then build and check the project:

```sh
npm install --prefix frontend
npm run build --prefix frontend
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo build --release
```

If `cargo clippy` is unavailable in a rustup installation, add it with `rustup component add clippy`.

The default tests do not open a microphone, do not download models, and do not need the network after crates are fetched.

## Run Echo

Echo ships as one binary, `echo-desktop`. With no arguments it opens the desktop app (Home, History, Dictionary, Settings) and stays available from the tray when its window is closed.

```sh
cargo run -p echo-desktop
./target/release/echo-desktop
```

Install it on your `PATH`:

```sh
install -Dm755 target/release/echo-desktop ~/.local/bin/echo-desktop
```

Make sure `~/.local/bin` is on your `PATH`.

## Dictation subcommands

Compositor shortcuts and hold-to-talk use subcommands on the same binary:

```sh
./target/release/echo-desktop rec --once
./target/release/echo-desktop rec --toggle
./target/release/echo-desktop rec --hold
./target/release/echo-desktop --hud-demo
```

`echo-desktop rec --once` records for three seconds, then moves the session from Recording to Transcribing. Set `ECHO_RECORD_SECONDS` to change the duration (up to 60 seconds), or set `ECHO_AUDIO_FIXTURE` to a 16 kHz WAV if you have no mic. Bind a compositor key to `echo-desktop rec --toggle` when evdev is not readable.

`echo-desktop rec --toggle` is intended for compositor shortcuts on Wayland. The first invocation starts recording; invoke it again to stop, transcribe, and insert at the focused cursor. It stops automatically after 60 seconds if the second invocation never arrives.

`echo-desktop rec --hold` waits for the hold key, records while it is down, and inserts on release, looping until you press Ctrl+C. The default key is Right Ctrl; set `ECHO_HOLD_KEY` to change it (for example `ECHO_HOLD_KEY=RightShift`). It reads keys from `/dev/input`, so add yourself to the input group first: `sudo usermod -aG input $USER`, then log out and back in. Without that access it exits with a hint and you should use the toggle instead.

While recording, Echo shows a click-through animated capsule near the bottom of the screen. It disappears before transcription and never takes keyboard focus. The capsule is X11-only; on a Wayland session without XWayland there is no HUD, and the desktop app is the recording indicator. Set `ECHO_HUD=off` to disable it.

### GNOME and Zorin OS global shortcut

Install `echo-desktop`, then open **Settings → Keyboard → View and Customize Shortcuts → Custom Shortcuts** and add:

- Name: `Echo Dictation`
- Command: `echo-desktop rec --toggle`
- Shortcut: `Super+Alt+Space` (or another unused combination)

Press the shortcut once to start recording and again to stop. GNOME keeps focus in the current application, so Echo inserts the transcript at its active cursor.

Models live under `$XDG_CACHE_HOME/echo` (normally `~/.cache/echo`) or `ECHO_MODEL_DIR`. Echo does not download models or engine binaries. For real transcription, configure either:

- Whisper: put `whisper-cli`, `whisper-cpp`, or `whisper` on `PATH`; put `ggml-base.en.bin`, `base.en.bin`, or `ggml-base.en.gguf` in the model directory; and set `ECHO_ENGINE=whisper`.
- Parakeet: put `sherpa-onnx-offline` or `sherpa-onnx` on `PATH`; put `tokens.txt`, the encoder, decoder, and joiner ONNX files in `parakeet-tdt-0.6b-v3/` below the model directory; and set `ECHO_ENGINE=parakeet`.

If the selected engine or its model is missing, recording ends with `EngineMissing`. With no `ECHO_ENGINE` setting, Echo picks the first installed real engine (Parakeet, then Whisper) and fails with `EngineMissing` when neither is installed. The deterministic fake engine runs only when you set `ECHO_ENGINE=fake`; it transcribes any non-silent audio as `claude code` and exists for smoke tests.

Dictionary and history live under `$XDG_DATA_HOME/echo`, or `$HOME/.local/share/echo`. Tests override that with `ECHO_DATA_DIR`. Use the desktop app's History and Dictionary views to browse and edit them.

Cleanup defaults to rules mode. It drops standalone um and uh, then capitalizes and adds ending punctuation. Set `ECHO_CLEANUP=off` to skip that pass. `ECHO_CLEANUP=local:binary` runs a stdin/stdout program on `PATH`.

## Status file

The recording process writes `$XDG_DATA_HOME/echo/status` as the session moves, including its pid. The desktop app reads that file; an active state whose writer has died reads as Idle, and a Failed state stays visible until the next session starts.

## Install the desktop entry

Install the `.deb` from the [GitHub Releases](https://github.com/ddv1982/echo/releases) page. The package installs `echo-desktop`, `Echo.desktop`, and the `echo-desktop` icons.

To add a menu entry from a source build, put `echo-desktop` on `PATH`, then:

```sh
mkdir -p ~/.local/share/applications
cp packaging/Echo.desktop ~/.local/share/applications/Echo.desktop
mkdir -p ~/.local/share/icons/hicolor/scalable/apps
cp assets/icons/echo-app.svg ~/.local/share/icons/hicolor/scalable/apps/echo-desktop.svg
for size in 32 128 256 512; do
  mkdir -p ~/.local/share/icons/hicolor/${size}x${size}/apps
  cp "src-tauri/icons/${size}x${size}.png" \
    ~/.local/share/icons/hicolor/${size}x${size}/apps/echo-desktop.png
done
update-desktop-database ~/.local/share/applications
gtk-update-icon-cache ~/.local/share/icons/hicolor
```

`packaging/Echo.desktop` runs `echo-desktop` and sets `Icon=echo-desktop`.

## Inject

On X11 the cascade is `xdotool type`, then clipboard plus Ctrl+V, then restore the clipboard. On Wayland it uses `ydotool` or `wtype` when those tools exist. A log line that says the insert worked is not enough. `cargo test -p echo --test inject_linux` types a nonce into a widget this repo compiles and reads that nonce back.

## Live checks

These stay ignored until you have hardware or cached models.

```
ECHO_LIVE_MIC=1 cargo test -p echo --test record_once -- --ignored
cargo test -p echo --test transcribe_fixture -- --ignored
cargo test -p echo --test compare_engines -- --ignored
```
