# Phase 9: settings IPC

Back to [overview](overview.md).

## Goal

The webview can read and write the config file. No UI yet.

## Changes

**`src-tauri/src/main.rs`.** Two commands added to the `generate_handler!` block at `:275-283`:

- `get_settings() -> Result<Settings, String>`
- `set_settings(settings: Settings) -> Result<Settings, String>`

`set_settings` returns the settings as re-read after the write, not the input echoed back. The caller then renders what is actually stored, so a value the backend clamped or rejected shows up immediately rather than appearing to have been accepted. That also makes the command idempotent (**principle-make-operations-idempotent**).

No capability change needed. `core:default` already permits `invoke`.

**Invalidate the health cache on write.** `health_snapshot` (`:45-65`) holds engine, injection, and microphone readiness for 10 seconds, while `cleanupName` and `hudEnabled` recompute on every call (`:111-112`). Change the engine and today the UI would update the cleanup row instantly and the engine row up to 10 seconds later. That reads as a bug and users will report it as one.

**`frontend/src/types.ts`.** A `Settings` interface matching the Rust struct. The Rust wire structs all carry `#[serde(rename_all = "camelCase")]`; follow that.

**`frontend/src/tauri.ts`.** Wrappers plus preview fixtures. This is required, not optional. `isTauri()` checks `window.__TAURI_INTERNALS__`, which jsdom never defines, so every wrapper takes its fallback branch under Vitest. A wrapper with no preview branch calls `invoke` with no Tauri present and the test fails. Make `setSettings` mutate the module-level fixture the way `addDictionaryEntry` does at `:84-89`, so tests can observe a real round-trip with no mocking.

**Also add a `settings_path` field to `AppStatus`.** Tell the user where the file lives. A local-first app should never make its own configuration hard to find, and it turns a bug report into a paste (**principle-experience-first**).

## Data structures

`Settings` is the serialised projection of phase 7's `Config`, plus the resolved effective value and its source for each field.

`{ engine: { value, effective, source }, ... }` where `source` is `env | file | default`. That third field is what lets phase 15's transparency panel say "engine is Whisper because `ECHO_ENGINE` is set" instead of showing a dropdown that silently does nothing. An environment variable overriding a saved setting, with the UI giving no hint, is the worst outcome available here and it is entirely avoidable by putting the source on the wire.

## Verification

**Static.** `cargo test --workspace` for the commands against a temp `ECHO_CONFIG_DIR`. `npm run test --prefix frontend` for the wrappers. `npm run build --prefix frontend` runs `tsc --noEmit`, which is the real check that the TypeScript interface matches the Rust struct.

**Runtime.** Via **control-ui**, from the webview devtools console since there is no UI yet. Call `getSettings`, call `setSettings` with a changed engine, and confirm three things: the return value reflects the write, `~/.config/echo/config.json` on disk changed, and a subsequent `getAppStatus` reports the new engine **immediately** rather than up to 10 seconds later. That last one is the health-cache assertion and it is the one that will be forgotten.

Then set `ECHO_ENGINE` in the environment the app was launched from and confirm `getSettings` reports `source: "env"` for that field.
