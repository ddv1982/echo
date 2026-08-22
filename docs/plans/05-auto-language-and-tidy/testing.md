# Testing: auto language and tidy

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
| 1. Auto by default | Resolution rule table tests (unset+multilingual → Auto, unset+.en → pinned English, configured wins, `.en`+German still refused); auto-detected Japanese gains no ASCII period | control-cli: Dutch fixture through `rec --once` with default config yields Dutch or a visibly low-confidence chip; `ECHO_LANGUAGE=nl` yields Dutch with no detection line; before/after transcripts on the PR |
| 2. Tidy-up | Suite plus drift check over the reduced raster set; availability payload omits Fake by default and includes it under `ECHO_SHOW_FAKE` | control-ui: selector shows no Fake; `ECHO_SHOW_FAKE=1` brings it back; screenshots both themes |
| 3. Settings IA | Component tests pin the tiers: General has exactly Microphone, Language, Model quality, Theme; Advanced collapsed by default; env-locked fields locked in Advanced | control-ui: both tiers at 920x680 in both themes; keyboard-only walkthrough including disclosure expand |

## Surfaces with no control skill

Unchanged from plan 04: the tray (panel screenshots on real hardware) and the X11 HUD (`--hud-demo` under Xvfb). Neither is touched by this plan.

## Live checks

The ignored live tests stay as they are. Phase 1 adds the closing check to run once on hardware: with a multilingual model installed and no language configured, dictate in a non-English language and confirm the transcript comes back in that language with the detected chip showing the code and probability.
