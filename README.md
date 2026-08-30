# Echo

Private local dictation for Linux. Press Super+Alt+Space, speak, then press it again to transcribe and insert cleaned text at the cursor. Audio never leaves the machine.

The first-build plan is [docs/plans/01-echo/overview.md](docs/plans/01-echo/overview.md).

## Download

[GitHub Releases](https://github.com/ddv1982/echo/releases) are the supported downloads. A Git tag without a corresponding GitHub Release marks source history only.

The current release workflow publishes four application files: a Debian package, an RPM package, an AppImage, and the raw `echo-desktop` binary. It also publishes a Cargo and npm SBOM, both license texts, and `SHA256SUMS`. GitHub records build-provenance attestations for these files. Older GitHub Releases may contain fewer files.

To verify a release that contains `SHA256SUMS`, download all its assets into an empty directory and run:

```sh
sha256sum --check --strict SHA256SUMS
```

Nightly Linux artifacts from the [release workflow](https://github.com/ddv1982/echo/actions/workflows/release.yml) are test builds. Maintainers should follow the [release runbook](docs/RELEASING.md).

## Build

You need Rust 1.88 or newer and Node.js 22 or newer. On Ubuntu, Debian, Zorin OS, and their derivatives, install the native build and runtime dependencies with:

```sh
sudo apt update
sudo apt install build-essential clang libclang-dev pkg-config libasound2-dev \
  libpipewire-0.3-dev libpulse-dev libwebkit2gtk-4.1-dev \
  libdbus-1-dev libayatana-appindicator3-dev \
  xdotool
```

Then build and check the project:

```sh
npm install --prefix frontend
npm run build --prefix frontend
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo build --release
./scripts/verify-recording-limit.sh
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

Compositor shortcuts use subcommands on the same binary:

```sh
./target/release/echo-desktop rec --once
./target/release/echo-desktop rec --toggle
./target/release/echo-desktop --hud-demo
```

`echo-desktop rec --once` records until the maximum recording length, then moves the session from Recording to Transcribing. The default and hard ceiling are ten minutes. Change the limit in Settings or with `ECHO_RECORD_SECONDS`; values clamp to 1 through 600 seconds. Set `ECHO_AUDIO_FIXTURE` to a 16 kHz WAV if you have no mic.

`echo-desktop rec --toggle` is intended for compositor shortcuts on Wayland. The first invocation starts recording; invoke it again to stop, transcribe, and insert at the focused cursor. It uses the same maximum recording length as `rec --once` and can stop sooner on the second invocation.

## File transcription

Transcribe a WAV file without opening the desktop, microphone, HUD, injector, or recording stores:

```sh
echo-desktop transcribe speech.wav
echo-desktop transcribe speech.wav --engine whisper --model small --language de
echo-desktop transcribe speech.wav --format json --output result.json
echo-desktop transcribe speech.wav --raw
echo-desktop languages --engine whisper --format json
echo-desktop languages --engine parakeet
```

Text output defaults to cleaned text on stdout. `--output -` also selects stdout, while any other value is the exact destination path. `--raw` selects uncleaned text and cannot be combined with JSON. Diagnostics use stderr. CLI overrides apply to one run and do not rewrite Settings.

Auto engine selection uses Whisper when a pinned language or CLI model requires it. Parakeet accepts automatic language selection only. Whisper receives up to 32 recent dictionary spellings as recognition hints, then Echo still applies the dictionary after recognition.

While recording, Echo shows a click-through capsule near the bottom of the screen with live microphone levels; it stays up through transcription and ends on a Done or Failed state. It never takes keyboard focus. The capsule is X11-only; on a Wayland session without XWayland there is no HUD, and the desktop app is the recording indicator. Set `ECHO_HUD=off` to disable it.

### Wayland global shortcuts

Echo registers one fixed Super+Alt+Space toggle when the desktop provides the GlobalShortcuts portal. GNOME 46 and older do not provide that portal. On those releases, Echo Settings inspects GNOME's `Echo Dictation` custom shortcut and shows **Set up GNOME shortcut** or **Repair GNOME shortcut** when an explicit write is safe. Echo never edits desktop settings at startup. On click, it rechecks the complete shortcut state and aborts if anything changed, then applies the Echo fields and merged path list as one atomic dconf changeset. Keep GNOME Settings closed during that click: dconf has no compare-and-swap operation, so a different process writing in the final interval after Echo's recheck remains a last-writer-wins race.

The GNOME fallback uses the running executable's absolute path, for example `/usr/bin/echo-desktop rec --toggle`, rather than a PATH lookup that an old source install could shadow. Press the shortcut once to start recording and again to stop. GNOME keeps focus in the current application, so Echo inserts the transcript at its active cursor.

On another Wayland compositor without GlobalShortcuts support, Settings reports that automatic setup is unsupported and shows the exact command and key combination to add in that compositor's keyboard settings. Echo does not silently grant raw-input access or mutate compositor configuration.

### Shortcut diagnostics and evidence

The shortcut setup row distinguishes the desktop portal, X11, a ready GNOME custom shortcut, manual compositor setup, and failed registration. **Test shortcut** accepts only an activation from the active native or GNOME shortcut route; a GUI or tray recording does not count.

| Source | Acceptance evidence | Claim boundary |
| --- | --- | --- |
| GlobalShortcuts portal | The production ashpd path registers, binds, routes activation/deactivation, handles changed triggers, and closes its session against a private D-Bus service implementing the official Registry, Request, Session, and GlobalShortcuts contracts. | Protocol and Echo state-machine behavior only; this does not certify a real compositor's consent dialog or UI. |
| X11 | The production global-hotkey path runs in nested Xephyr. Hardware-level ydotool input reaches the toggle handler while another inner application owns focus; a separate check proves conflict rejection and unregister cleanup. | Native X11 routing and ownership, not Wayland behavior. |
| GNOME 46 Wayland | This host proves the GlobalShortcuts interface is absent, the production status IPC exposes explicit setup/repair, and repair changes only the confirmed Echo-owned custom binding. | Older-GNOME fallback only; desktop settings are never changed at startup. |

Models live under `$XDG_CACHE_HOME/echo` (normally `~/.cache/echo`) or `ECHO_MODEL_DIR`. On Linux x86_64, Home and Settings can install a complete managed setup without a package manager. Recommended uses multilingual Base Q5_1 below 8 GiB RAM or when memory is unknown, and the 547 MiB Large v3 Turbo Q5_0 model on machines with at least 8 GiB. Small remains an explicit advanced and compatibility choice. Parakeet setup installs the pinned sherpa-onnx runtime and its four model files.

Managed downloads resume after cancellation or interruption, check free disk space before HTTP, verify SHA-256, extract only catalogued archive payloads, and activate one immutable component generation at a time. Echo hashes every managed file on the first check in each process. On Linux filesystems that update inode metadata after writes, Echo also rehashes when the file's identity or metadata changes. An explicit Verify always hashes every file. These checks detect payload changes across process restarts, but they do not defend against an active writer that runs under the same account as Echo. Repair replaces missing or corrupt generations. Remove deletes only files recorded under Echo's managed directory. System runtimes on `PATH` and models copied into the existing flat cache remain external inputs; Echo labels and uses them but never repairs or removes them.

Settings stores the selected microphone by CPAL device ID and keeps its last label for disconnected-device messages. `Follow system default` remains distinct from pinning today's default. Equal labels remain separate, and the primary Test action never falls back to another device. A missing selection offers a separate fallback test.

If the selected engine or its model is missing, recording ends with `EngineMissing`. Auto picks the first installed real engine (Parakeet, then Whisper) and fails with `EngineMissing` when neither is installed. The deterministic fake engine runs only when you set `ECHO_ENGINE=fake`; it transcribes any non-silent audio as `claude code` and exists for smoke tests, so it stays out of the Settings selector unless `ECHO_SHOW_FAKE=1` is set.

With no `ECHO_WHISPER_MODEL` setting, Whisper runs the best installed model: multilingual over `.en`, then the larger family. Pin one with `ECHO_WHISPER_MODEL=small` or the `whisper_model` config key.

Model comparisons can be rerun through the shipping CLI with `scripts/benchmark-stt.py`. See `benchmarks/stt/README.md` for the manifest format and Small, Large v3 Turbo Q5_0, and Parakeet example.

With a multilingual model, Echo detects the language automatically. Set `ECHO_LANGUAGE=de` (or the `language` config key) to pin any of the 100 languages whisper.cpp supports; pinning skips detection's extra encoder pass, and after a confident auto-detection Settings offers to pin the detected language in one click. An English-only (`.en`) model pins English, the only thing it can do, and combining one with a non-English language or `auto` is refused before recording, because whisper-cli would silently transcribe English and exit 0. Parakeet identifies its 25 supported languages automatically and reports none.

Dictionary and history live under `$XDG_DATA_HOME/echo`, or `$HOME/.local/share/echo`. Tests override that with `ECHO_DATA_DIR`. Use the desktop app's History and Dictionary views to browse and edit them.

Cleanup defaults to rules mode. It drops standalone um and uh, then capitalizes and adds ending punctuation. Set `ECHO_CLEANUP=off` to skip that pass. `ECHO_CLEANUP=local:binary` runs a stdin/stdout program on `PATH`.

## Status file

The recording process writes `$XDG_DATA_HOME/echo/status` as the session moves, including its pid and the limit snapped for an active recording. The desktop app reads that file; an active state whose writer has died reads as Idle, and a Failed state stays visible until the next session starts.

## Install the desktop entry

Install the `.deb` from the [GitHub Releases](https://github.com/ddv1982/echo/releases) page. The package installs `echo-desktop`, `io.github.ddv1982.echo.desktop`, and the `echo-desktop` icons.

To add a menu entry from a source build, put `echo-desktop` on `PATH`, then:

```sh
mkdir -p ~/.local/share/applications
cp packaging/Echo.desktop ~/.local/share/applications/Echo.desktop
mkdir -p ~/.local/share/icons/hicolor/scalable/apps
cp assets/icons/echo-app.svg ~/.local/share/icons/hicolor/scalable/apps/echo-desktop.svg
mkdir -p ~/.local/share/icons/hicolor/symbolic/apps
cp assets/icons/echo-symbolic.svg ~/.local/share/icons/hicolor/symbolic/apps/echo-desktop-symbolic.svg
for size in 32 128 256 512; do
  mkdir -p ~/.local/share/icons/hicolor/${size}x${size}/apps
  cp "src-tauri/icons/${size}x${size}.png" \
    ~/.local/share/icons/hicolor/${size}x${size}/apps/echo-desktop.png
done
update-desktop-database ~/.local/share/applications
gtk-update-icon-cache ~/.local/share/icons/hicolor
```

`packaging/Echo.desktop` runs `echo-desktop` and sets `Icon=echo-desktop`.

## Upgrading from a source install

The manual install above puts `echo-desktop` in `~/.local/bin`, which precedes `/usr/bin` in PATH on Ubuntu and GNOME. If you later install the `.deb`, the stale source build keeps winning, and the desktop entry keeps launching it. Remove the source install when you switch to the package.

The easy way: Echo's Home view warns when another `echo-desktop` on PATH shadows the running one, and the warning's **Remove old copies** button deletes the stale binaries and the user-local leftovers for you. The manual equivalent:

```sh
rm ~/.local/bin/echo-desktop
rm ~/.local/share/applications/Echo.desktop
rm ~/.local/share/icons/hicolor/scalable/apps/echo-desktop.svg
rm ~/.local/share/icons/hicolor/symbolic/apps/echo-desktop-symbolic.svg
for size in 32 128 256 512; do
  rm -f ~/.local/share/icons/hicolor/${size}x${size}/apps/echo-desktop.png
done
update-desktop-database ~/.local/share/applications
gtk-update-icon-cache ~/.local/share/icons/hicolor
```

A second launch of a packaged build that replaced the binary restarts into the new build instead of opening a duplicate tray, and a new build terminates pre-0.3.0 processes at startup. Confirm what is running with `echo-desktop --version` or the version readout in Settings.

The brand colors, declared for Flathub metainfo when packaging catches up: light `#f8f1de`, dark `#1c1c1c`. The mark's bars run a cream-to-amber gradient (`#f8f1de` into `#e2a23a`) on a dark tile (`#282828` into `#121212`), and the tray glyph is the same mark reduced to three dual-tone bars so it reads on light and dark panels alike.

## Inject

On X11 the cascade is `xdotool type`, then clipboard plus Ctrl+V. On Wayland it uses `ydotool` or `wtype` when those tools exist, with the same clipboard fallback. Clipboard fallback leaves the transcript on the clipboard because restoring the previous value before the target requests it can paste the wrong text; direct typing never changes the clipboard. A log line that says the insert worked is not enough. `cargo test -p echo --test inject_linux` types a nonce into a widget this repo compiles and reads that nonce back.

## Live checks

These stay ignored until you have hardware or cached models.

```
ECHO_LIVE_MIC=1 cargo test -p echo --test record_once -- --ignored
cargo test -p echo --test transcribe_fixture -- --ignored
cargo test -p echo --test compare_engines -- --ignored
```

## License

Echo is available under either the [MIT License](LICENSE-MIT) or the [Apache License 2.0](LICENSE-APACHE).
