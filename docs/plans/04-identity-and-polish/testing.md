# Testing: identity and polish

Back to [overview](overview.md).

## Project-level

Run per phase and at completion. The frontend build is first because every cargo command depends on it:

```sh
npm ci --prefix frontend
npm run build --prefix frontend
npm run test --prefix frontend
npm run lint --prefix frontend
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release
```

CI runs this list plus the icon drift check (`cargo run -p xtask`, then `git diff --exit-code` over the generated rasters), so a green PR is the project-level check.

## Per phase

| Phase | Static | Runtime |
| --- | --- | --- |
| 1. The mark | `cargo test -p xtask`: RGBA, transparent corners, dual-background tray contrast. Drift check idempotent | Panel screenshots at 22 and 24 px on light and dark shell themes; app grid at 64 px; 512 px render |
| 2. Model catalog | Table-driven filename tests incl. `-q8_0` and ignored `-tdrz`; best-installed ranking test | `rec --once` with three real models selects the predicted one; explicit config beats the ranking |
| 3. Model picker | Frontend tests: select appears only for Whisper, unavailable engine shows its reason, readout renders fixture values | control-ui: readout's model path is the file on disk; rename the file and the failure names it. Screenshots both themes |
| 4. Language model | Table has 100 entries, `en` id 0, `yue` id 99; `-l` argument construction; refusal before spawn on `.en`; no `--task translate` anywhere; Japanese `。` gains no ASCII period | control-cli: pinned German, refused German on `base.en`, auto-detected German in `result.language`, auto-vs-pinned latency in the PR |
| 5. Language picker | Frontend tests: Auto first, 100 entries plus Auto for multilingual, English only for `.en`, detected chip renders, incompatibility warning renders | control-ui: warning before recording for `.en` plus German; keyboard-only `ger` jump; screenshots both themes |
| 6. Model download | URL construction per offer; hash mismatch deletes the temp file; re-run is a no-op; cancel leaves no partial; local HTTP fixture, never the network in CI | control-ui: VAD model end to end, progress and verifying states visible, mid-download cancel, corrupted-hash rejection, then the closing loop: download `small`, pick German, dictate German |
| 7. The HUD | Level-to-bar mapping, smoothing constants, state transitions, compositor-detection fallback | Xvfb plus `--hud-demo` screenshot per state; live `rec --toggle` on hardware; compositor-present screenshot via xcompmgr or picom where available |
| 8. Beauty pass | Component tests for stats, checklist, level bar states | control-ui: every view at 920x680 in both themes, live-bars hero screenshot, keyboard-only walkthrough, WebKitGTK-specific motion check |

## Surfaces with no control skill

Two, both carried over from earlier plans. The tray is drawn by libappindicator into the desktop panel and cannot be driven; phase 1 verifies it by screenshotting a real panel. The X11 HUD has no control skill; phase 7 verifies it with `--hud-demo` under Xvfb plus screenshots, the fallback [02-design-overhaul/testing.md](../02-design-overhaul/testing.md) established.

## Live checks

The ignored live tests from the README stay ignored until hardware and cached models exist:

```sh
ECHO_LIVE_MIC=1 cargo test -p echo --test record_once -- --ignored
cargo test -p echo --test transcribe_fixture -- --ignored
cargo test -p echo --test compare_engines -- --ignored
```

Phases 2, 4, and 6 add the closing-loop check each time one of them lands: a real dictation in a non-English language on a downloaded multilingual model, with the transcript landing at the cursor.
