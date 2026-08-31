# Architecture

Echo has a local Rust domain layer, a Tauri desktop boundary, and a React user
interface. The desktop process is the only bridge between the browser runtime
and local operating-system capabilities.

## Workspace map

| Path | Responsibility |
| --- | --- |
| `crates/echo-core` | Domain types, config, history, dictionary, recording state, and paths. |
| `crates/echo` | Audio, transcription engines, managed installation, injection, HUD, and recording orchestration. |
| `crates/echo-ipc` | Rust command, event, and payload contract exported to TypeScript. |
| `crates/ipc-gen` | Deterministic generator and drift check for the frontend IPC contract. |
| `src-tauri` | Desktop composition, command adapters, setup, status projection, shortcuts, tray, and CLI. |
| `frontend` | React views, feature controllers, desktop API adapters, and generated IPC types. |
| `crates/xtask` | Repository maintenance tasks such as icon generation. |

## Dictation flow

1. A tray, UI, CLI, portal, GNOME, or X11 action requests a recording toggle.
2. The recording layer captures the selected microphone and writes session
   state under the XDG data directory.
3. The configured local engine transcribes the captured audio. Whisper can use
   the managed CPU runtime or an explicitly selected GPU runtime; Parakeet uses
   its managed local runtime.
4. Echo applies personal Dictionary replacements to the engine transcript.
5. The injector types or pastes the result into the active application.
6. The desktop projects status and refreshes History and Dictionary after the
   session returns to Idle.

## Desktop boundary

Tauri command functions are thin adapters. Settings owns serialized config
writes and invalidates the status cache after a successful save. Status owns
health caching and the `AppStatus` projection. Focused command modules own
devices, library data, recording, settings, shortcuts, status, and system
operations.

The Settings boundary returns one `SettingsSnapshot`. The snapshot keeps saved
preferences, the resolved next transcription, setup readiness, and previous-run
telemetry separate. `SettingsChange` updates one config field or performs the
explicit Whisper GPU transition, then returns a new snapshot.

Shortcut policy is one subsystem behind a small facade. It owns portal and X11
listeners, the older-GNOME fallback, retry state, and shutdown cleanup. Desktop
startup reconciles one listener; shutdown cancels and joins it.

## IPC contract

Rust is authoritative for serialized commands, events, and payloads. `ipc-gen`
writes the TypeScript contract and command manifest. CI regenerates them and
fails on drift. Frontend code imports generated types and calls a `DesktopApi`
interface rather than constructing command strings throughout the UI.

The production adapter invokes Tauri. The preview adapter is isolated to the
browser development graph and cannot enter the production bundle.

## Frontend ownership

`App.tsx` composes navigation and feature surfaces. Shared status, theme,
history, dictionary, and error state live in the app controller. Home and
Settings own their subscriptions and device/setup lifetimes. Settings changes
are serialized, and stale setup refreshes cannot replace a newer snapshot.

Serial polling never overlaps requests and stops with component disposal.
Subscriptions await their unlisten handle and dispose it even when unmount
races setup.

## Release boundary

Tagged builds publish the raw binary, Debian package, RPM, AppImage, the MIT
license, a CycloneDX SBOM, and `SHA256SUMS`. CI verifies the complete
staged set before upload and creates build-provenance attestations. Third-party
workflow actions are pinned to full commit SHAs.

See [RELEASING.md](RELEASING.md) for the operator contract.
