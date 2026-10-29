# Topbar scroll glitch

Scrolling down and back to the top shows visual glitches in the top bar and the
Home/History/Dictionary/Settings navigation: content shows through (reported
with the mouse wheel).

- [x] Reproduce the glitch in WebKitGTK (the engine the Tauri app uses) with captured frames
- [x] Identify the root cause from the reproduction
- [x] Failing test: switching views resets the scroll position of `main`
- [x] Shell layout: only `main` scrolls, topbar and nav are not sticky; scroll reset targets `main`
- [x] Same reproduction no longer shows the glitch
- [x] e2e fit check also covers horizontal overflow inside `main`
- [x] Frontend CI checks pass locally (typecheck, lint, tests, responsive e2e)
- [ ] PR filed, reviews addressed, required checks green
- [ ] PR merged

## Findings

- Reproduced only on native Wayland (in-process capture of the frames WebKit
  hands to GTK while sending smooth wheel deltas). XWayland and Xvfb never
  showed it.
- Baseline: 12, 27 (920 wide) and 22 (1280 wide) of 420 frames had the sticky
  topbar or nav drawn out of place with content showing through.
- `content-visibility: visible` on transcript rows: 11 and 20 of 420, so not the cause.
- Only `main` scrolling (bars not sticky): 0 of 420 in three runs, at 920 and 1280.
- The user confirmed on their machine: the glitch shows in the old build and not in the fixed one.
- Screen recordings of the user's own mouse scrolling on Home (GNOME screencast,
  bar region checked frame by frame): old build 21 of 1558 frames with the top
  bar displaced 15 to 30px; fixed build 0 of 1515.
