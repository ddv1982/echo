# Troubleshooting

## The shortcut does not work

Open Settings and read the shortcut status before adding another binding.

- Desktops with the GlobalShortcuts portal use Echo's fixed
  **Super+Alt+Space** registration.
- GNOME 46 and older need an explicit custom shortcut. Use **Set up GNOME
  shortcut** or **Repair GNOME shortcut** only when Echo offers it.
- Other Wayland compositors may require a manual binding to the exact command
  shown in Settings.
- X11 uses a native global grab. A conflicting application can prevent
  registration.

Use **Test shortcut** in Settings. It accepts only an activation from the active
shortcut route; starting a recording from the UI or tray does not count.

## Echo records from the wrong microphone

Choose the device in Home or Settings and run its test. **Follow system
default** remains dynamic; choosing a named device pins that device ID. Devices
with identical labels remain separate. If a pinned device disconnects, Echo
reports its last known label instead of silently using another input.

## A speech engine is missing

Use the setup or repair action on Home or Settings. Managed downloads resume
after interruption, verify SHA-256 before extraction, and activate only a
complete generation. Models normally live under `~/.cache/echo` or
`$XDG_CACHE_HOME/echo`. Set `ECHO_MODEL_DIR` only when you intentionally manage
models elsewhere.

For GPU-specific fallback messages, see [GPU runtime](gpu-runtime.md).

## Text is not inserted

On X11, Echo tries direct typing and then clipboard paste. On Wayland it uses
`ydotool` or `wtype` when available, with clipboard paste as a fallback. Install
the input tool supported by your compositor and confirm it can type into the
target application.

Clipboard fallback leaves the transcript on the clipboard. Restoring the prior
clipboard too early can make the target paste the wrong value.

## An old source build keeps opening

`~/.local/bin` commonly precedes `/usr/bin`. A previous manual install can
therefore shadow a newer package. Home reports the shadowing path and offers
**Remove old copies**. Confirm the active binary with:

```sh
command -v echo-desktop
echo-desktop --version
```

## Find local state

- settings: the path shown in **Settings → Advanced**;
- models and managed runtimes: `$XDG_CACHE_HOME/echo` or `~/.cache/echo`;
- history, dictionary, and session status: `$XDG_DATA_HOME/echo` or
  `~/.local/share/echo`.

The status file records the active session PID and recording limit. A recording
whose writer process died reads as Idle. A Failed state remains visible until
the next session begins.

## Build dependencies

Echo requires Rust 1.88 or newer and Node.js 22 or newer. On Ubuntu, Debian,
Zorin OS, and derivatives:

```sh
sudo apt update
sudo apt install build-essential clang libclang-dev pkg-config libasound2-dev \
  libpipewire-0.3-dev libpulse-dev libwebkit2gtk-4.1-dev libdbus-1-dev \
  libayatana-appindicator3-dev xdotool
```

Then run the source build commands from the repository root:

```sh
npm ci --prefix frontend
npm run build --prefix frontend
cargo build --release
```
