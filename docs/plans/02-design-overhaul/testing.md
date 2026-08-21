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

1. The real Tauri webview (`cargo run -p echo-desktop`), which is WebKitGTK. Required at least once per phase because font features, `backdrop-filter`, and color rendering differ from Chromium.
2. The Vite dev server in a browser for fast iteration, with `ECHO_ENGINE=fake` and `ECHO_DATA_DIR` pointed at a scratch directory so history and dictionary flows run end to end without hardware.

Per phase checks are listed in each phase file. The cross-phase constants:

- Both themes, every touched view, at the 920x680 default window and at the 760x560 minimum.
- Keyboard-only pass: tab order and the double-ring focus visible on every interactive element (from phase 5 on).
- Before and after screenshots attached to every visual PR.
- `prefers-reduced-motion: reduce` still suppresses all animation.

## Runtime, HUD (phase 11)

No control skill exists for raw X11 windows; flagged per the plan playbook. Manual fallback: `xvfb-run ./target/release/echo-desktop --hud-demo`, capture with `xwd`/`import`, compare against the dark theme screenshot. `cargo test -p echo-desktop --test cli_hud_demo` stays green.

## End-to-end acceptance, after phase 11

With the fake engine, run one full dictation loop from the desktop app (toggle record, speak or feed `ECHO_AUDIO_FIXTURE`, watch HUD, confirm insertion into history) and confirm the window and the capsule read as one design. This is the prove-it-works gate for the overhaul as a whole.
