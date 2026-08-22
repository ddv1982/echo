# Phase 1: the mark

Back to [overview](overview.md).

## Goal

One well-drawn mark in both roles: a redesigned app icon, and a tray glyph that is visibly the same mark, rendered with real alpha, readable on light and dark panels alike.

This phase supersedes plan 03 phase 5's decision to keep the old geometry. The plumbing that phase built (SVG masters, the `xtask` generator, RGBA output, the CI drift check) stays and is the reason this phase is small.

## Changes

**`assets/icons/echo-app.svg`, redesigned.** The metaphor stays: voice bars resolving into a period, speech becoming text. It is the right metaphor, it matches the HUD's waveform, and plan 03's small-size measurements are the only thing wrong with it. What changes is the execution, following the [GNOME icon guidance](https://blogs.gnome.org/tbernard/2019/12/30/designing-an-icon-for-your-app/) and the [Flathub quality guidelines](https://docs.flathub.org/docs/for-app-authors/metainfo-guidelines/quality-guidelines):

- Keep the 1024 canvas and the rounded-square tile silhouette. Give the tile a subtle vertical gradient instead of flat `#1c1c1c`, and give the bars a warm gradient (cream into amber) so the mark has a temperature the grayscale app does not. Curved surfaces may carry gradients; keep everything geometric.
- Tune bar heights and spacing for recognition at 64 px, not 128. The current five bars plus dot merge below 32 px, measured in plan 03's overview.
- Contrast is a hard requirement, not a hope: the tile must read on white and on black backgrounds. Flathub rejects icons that fail this.
- Produce two or three candidate masters, render each at 16, 22, 24, 32, 48, 64, 128, and 512 px through the existing generator, and attach the contact sheet to the PR. The choice is made from the renders, not from the SVG source.

**`assets/icons/echo-tray.svg`, same DNA, honest about size.** The user asked for the same icon with alpha in the tray. The measured fact from plan 03 is that the full five-bar mark does not fit: at 22 px the bar gaps are 0.79 device pixels and bars merge. So the tray glyph is the mark reduced to its readable essence, three bars at the same radius and proportions, on a fully transparent ground. To survive unknown panel colors it is dual-tone: a light fill with a dark keyline around the union silhouette, wide enough to register at 22 px. There is no panel-brightness API on Linux to do anything smarter ([tauri#3857](https://github.com/tauri-apps/tauri/issues/3857)); the dual-tone glyph is the maintainers' recommended answer. Draw it on a 24 px grid with integer geometry and scale up, never down.

**`assets/icons/echo-symbolic.svg`, new.** A single-color 16 px symbolic per the [GNOME integration guide](https://developer.gnome.org/documentation/guidelines/maintainer/integrating.html): three bars, 2 px strokes, outermost 1 px empty. GNOME uses it in notifications and shell chrome where a full-color icon is wrong.

**`assets/icons/echo.svg`, deleted.** Nothing references it. One app master, one tray master, one symbolic.

**`crates/xtask/src/main.rs`.** Two changes.

- Replace `tray_rasters_are_three_bars_on_clear_ground` with a contrast test that means something: composite each tray raster over pure white and over pure black, and assert the glyph's mean relative luminance differs from each background by at least 0.30. A near-white glyph on transparent ground, which is what ships today, fails this test on white. Keep the transparent-corners assertion.
- Add the new candidate masters to the raster table only after the PR picks one. The drift check in CI (`cargo run -p xtask` then `git diff --exit-code`) keeps masters and rasters honest without new machinery.

**Wiring, unchanged on purpose.** `src-tauri/tauri.conf.json` keeps its five bundle icons, `src-tauri/src/main.rs:511` keeps its embedded `tray-24.png`, `packaging/Echo.desktop` keeps `Icon=echo-desktop`, and the favicon keeps regenerating from the app master. Regenerating is the whole point of the generator.

**`README.md`.** The manual icon install snippet gains the symbolic icon line (`echo-desktop-symbolic.svg` into `hicolor/symbolic/apps`). Record the brand colors, light and dark variants, in the README's icon section; Flathub wants them declared when packaging catches up.

## Data structures

No new code structures. The design artifacts are the three SVG masters and the regenerated rasters. The contrast threshold lives in the xtask test as a named constant with a comment citing the backgrounds it composites over.

## Verification

**Static.** `cargo run -p xtask` is idempotent and the drift check passes. `cargo test -p xtask` covers: RGBA everywhere, transparent corners on app and tray rasters, and the new dual-background contrast assertion. The old three-bars opaque-pixel-count test is deleted, not weakened.

**Runtime.** Screenshot the tray on a real panel at 22 px and 24 px, once under a light shell theme and once under a dark one, and attach both pairs to the PR. Screenshot the GNOME app grid at 64 px and the icon at 512 px. The tray screenshots are the acceptance gate: if the glyph disappears against either theme, the keyline is wrong, and no amount of SVG review substitutes for the pixels.
