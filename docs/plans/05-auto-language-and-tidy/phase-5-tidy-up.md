# Phase 5: tidy-up

Back to [overview](overview.md).

## Goal

Remove what the repo no longer needs, and mark what already shipped. Subtraction before addition: this lands before the settings redesign so phase 6 restyles a smaller surface.

## Changes

**Plan status headers.** One line at the top of each plan overview in `docs/plans/`: 01, 02, and 04 shipped in full; 03 shipped except phases 16 and 20, which stay open and named. The header links the release that carried the work. No folders deleted; the plans are the audit trail.

**The Fake engine leaves the shipping selector.** `engine_availability` (`crates/echo/src/stt/mod.rs:221`) stops listing Fake as available unless `ECHO_ENGINE=fake` or `ECHO_SHOW_FAKE` is set, and the frontend's `ENGINE_OPTIONS` (`frontend/src/App.tsx:693-697`) drops the Fake button on the same condition, driven by the availability payload rather than a second hardcoded list. `ECHO_ENGINE=fake` keeps working for CLI smoke tests; the README keeps documenting it. The frontend tests that click the Fake button as their mutation target migrate to another setting (the cleanup segmented control is the same shape).

**Unreferenced tray rasters.** `tray-22.png`, `tray-32.png`, and `tray-48.png` leave the xtask raster table and the tree; only `tray-24.png` is consumed (`src-tauri/tauri.conf.json:30`, `src-tauri/src/main.rs:932`). The drift check keeps covering what remains.

**Dead dictionary hit tracking.** `Rewrite.hits` and `DictHit` go. The only reader is their own test (`crates/echo-core/src/cleanup.rs:180`); the test adjusts to assert on the rewritten text alone.

**README corrections.** "Echo does not download models or engine binaries" (`README.md:84`) is false since guided downloads shipped; the models paragraph points at the in-app offers and keeps the manual drop-in path for unlisted models.

**Considered and kept.** The three surviving hand-synced pairs (`resolve_engine`/`engine_summary`, `from_env`/`mode_name`, `hud_disabled`/`enabled`) stay: each is small, and each carries an agreement test that pins the projection. `config_dir` stays exported; trimming one path helper is not worth a diff. Dependencies need no changes; the manual review found every crate and npm package in use.

## Data structures

None. This phase deletes.

## Verification

**Static.** The project-level suite, plus `cargo run -p xtask` and the drift check over the reduced raster set. The Fake-engine frontend tests pass against the new mutation target, and a test pins that the availability payload omits Fake by default and includes it under `ECHO_SHOW_FAKE`.

**Runtime.** Via **control-ui**: the Settings engine selector shows Auto, Whisper, Parakeet and no Fake; with `ECHO_SHOW_FAKE=1` it reappears. Screenshot both themes at 920x680.
