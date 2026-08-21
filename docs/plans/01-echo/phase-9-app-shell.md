# Phase 9. App shell

Back to [overview](./overview.md).

## Goal

A real app you can start and forget. Tray icon, history window, dictionary editor, quit. Close the window and dictation keeps working, like the video's last demo in a Google Doc.

## Changes

`crates/echo/src/ui/history.rs` lists past `Transcript` rows.

`crates/echo/src/ui/tray.rs` owns the status icon and the open/quit menu.

`crates/echo-core/src/history.rs` persists sessions. Same data dir as the dictionary. Cap at a few thousand rows so the file stays boring.

Wire `main.rs` so one process runs hotkey, audio, STT, inject, HUD, and the tray. No second daemon unless Wayland forces `ydotoold`, which is already an external service.

## Data structures

`HistoryRow` is `{ id, text, raw, engine, started_at, infer_ms, inject: InjectReport }`.

`AppCommand` is `OpenHistory | OpenDictionary | Quit`. The session machine does not see these.

## Verification

Static. Workspace test and clippy. Persistence tests restart a store from disk.

Runtime. Linux. Launch the binary, dictate once via CLI if the tray grab is flaky, open history, confirm the row, quit, relaunch, confirm the row is still there. macOS. Same, then hide the window and inject into TextEdit to prove the process did not die with the window. No control skill. This is a recorded checklist in [testing.md](./testing.md).
