[Back to overview](overview.md)

# Testing

## Static, every phase

```sh
npm run build --prefix frontend   # runs tsc --noEmit, then vite build
npm run test --prefix frontend    # vitest + jsdom, App.test.tsx
npm run lint --prefix frontend
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

All five must pass before a phase PR opens. Phases 1 to 10 should not change Rust output; the cargo commands are the regression guard. Phase 11 is the inverse.

## Runtime, every frontend phase

Use the control-ui skill (from `cursor-team-kit`) against the running app. Two acceptable surfaces:

1. The real Tauri webview (`cargo run -p echo-desktop`), which is WebKitGTK. Required at least once per phase because font features, `backdrop-filter`, and color rendering differ from Chromium. This is also the only surface for end-to-end data-flow checks: launch it with `ECHO_ENGINE=fake` and `ECHO_DATA_DIR` pointed at a scratch directory so history and dictionary flows exercise the real backend without hardware.
2. The Vite dev server in a browser for fast styling iteration only. Outside Tauri, `frontend/src/tauri.ts` serves in-memory preview fixtures (`previewStatus`, `previewHistory`, `previewDictionary`) and never starts the Rust backend, so `ECHO_ENGINE` and `ECHO_DATA_DIR` are no-ops there. Interactions work against the fixtures, which is enough for visual checks but proves nothing about the input-to-output chain.

Per phase checks are listed in each phase file. The cross-phase constants:

- Both themes, every touched view, at the 920x680 default window and at the 760x560 minimum.
- Keyboard-only pass: tab order and the double-ring focus visible on every interactive element (from phase 5 on).
- Before and after screenshots attached to every visual PR.
- `prefers-reduced-motion: reduce` still suppresses all animation.

## Runtime, HUD (phase 11)

No control skill exists for raw X11 windows; flagged per the plan playbook. Manual fallback: `xvfb-run ./target/release/echo-desktop --hud-demo`, capture with `xwd`/`import`, compare against the dark theme screenshot. `cargo test -p echo-desktop --test cli_hud_demo` stays green.

## End-to-end acceptance, after phase 11

Two checks, because the fixture path and the HUD are mutually exclusive: `RecordingHud::start` returns without creating a HUD when `ECHO_AUDIO_FIXTURE` is set.

1. **Data flow.** With the fake engine and `ECHO_AUDIO_FIXTURE`, run one full dictation loop from the desktop app (toggle record, stop, confirm the transcript lands in history and Last transcript). No HUD appears on this path by design.
2. **HUD consistency.** Validate the capsule separately, via a live microphone capture where hardware exists or `--hud-demo` otherwise, and confirm the window and the capsule read as one design.

Together these are the prove-it-works gate for the overhaul as a whole.
