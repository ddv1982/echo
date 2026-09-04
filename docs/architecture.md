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

## Managed component integrity

`installer.rs` orchestrates managed installation. `store.rs` owns lifecycle and
locking, `payload.rs` owns payload projection, extraction plans, hashing, and the
process-local verification cache, and `filesystem.rs` owns containment, cleanup,
and resume calculations. `install/mod.rs` is the facade.

Legacy payload-adjacent `verified.json` is not a trust source. Shallow
verification immediately rejects a wrong file type, regular-file size or
permission mode, or symlink target. If those structural checks pass, a cold
process cache or changed fingerprint causes full hashing. The fingerprint covers
relative path, file type, full mode, size, device, inode, ctime, mtime, and
symlink target. Explicit Verify also forces a full hash. This detects persistent
mutation, but an active same-account writer is outside the boundary.

## Dictation flow

1. A tray, UI, CLI, portal, GNOME, or X11 action requests a recording toggle.
2. The recording layer captures the selected microphone and writes session
   state under the XDG data directory.
3. The configured local engine transcribes the captured audio. Whisper can use
   the managed CPU runtime or an explicitly selected GPU runtime; Parakeet uses
   its managed local runtime.
4. Echo applies personal Dictionary replacements to the engine transcript.
5. The injector types or pastes the result into the active application.
6. The desktop projects status. A persisted History row ID prompts the
   frontend to refresh History, including after insertion failure.

One cross-process lease covers capture, transcription, injection, and history
persistence. Normal CLI recording, toggle recording, voice training, and
upgrade takeover all use that lease. A fixed gate file supplies kernel-backed
exclusion. The token-bearing lock file remains a compatibility and diagnostic
record for older Echo processes.

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

Active status records include the writer PID and Linux process start time.
Readers reject zombies and reused PIDs. A successful History append publishes
the row ID through status, so the frontend refreshes History after both a
successful insertion and a failed insertion with recoverable text.

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
license, [`THIRD_PARTY.md`](../THIRD_PARTY.md), a CycloneDX SBOM, and
`SHA256SUMS`. CI verifies the complete staged set before upload and creates
build-provenance attestations.
The managed runtime and model archive bytes remain separate from application
assets, while their catalog URL, digest, license, supplier, and source
attribution are represented in the desktop SBOM. Third-party workflow actions
are pinned to full commit SHAs.

See [RELEASING.md](RELEASING.md) for the operator contract.
