# Phase 5: icon masters and generator

Back to [overview](overview.md).

## Goal

One source of truth for the Echo mark, and a script that renders every raster the project needs. No hand-exported PNGs.

## Changes

**Redraw the mark for small sizes. The current one does not survive a tray.** Measured on the actual pixels with a box downsample, which is more generous than the filters GTK panels use:

| Target | Bar width | Inter-bar gap | Bars resolved |
| --- | --- | --- | --- |
| 16 px | 1.13 px | 0.56 px | 4 of 5 |
| 22 px | 1.55 px | 0.79 px | 5 of 5 |
| 32 px | 2.25 px | 1.13 px | 5 of 5 |
| 48 px | 3.38 px | 1.69 px | 5 of 5 |

At 22 px and 24 px the arithmetic still finds five peaks, but the gaps are below one device pixel, so they render as grey mush. The gap only reaches a full pixel at 28.4 px. The mark was drawn for a launcher tile and it needs about 32 px to read.

So this phase produces **two** masters, not one scaled asset:

- **`assets/icons/echo-app.svg`.** The launcher tile. Keep the existing geometry, a `rx="220"` rounded square with five capsule bars and a dot below. It is good at 128 px and above and there is no reason to change it.
- **`assets/icons/echo-tray.svg`.** A distinct glyph for panels. Three bars instead of five, no tile, transparent background, and a single flat fill so a monochrome panel theme can recolour it. Design it at a 24 px grid and scale up, not the reverse.

Reconcile the colour discrepancy while doing this. The SVG uses `#1c1c1c` and `#f3efe6`; the PNG uses `#252628` and `#fcf3db`. Pick one pair, put it in both masters, and note in the PR which one and why.

**`scripts/build-icons.sh` or `scripts/build-icons.rs`, new.** Reads the two masters and writes every raster the project consumes. This is the artifact a reviewer reruns (**principle-build-the-lever**).

Outputs, sized to what the consumers actually need:

- `src-tauri/icons/{32x32,128x128,128x128@2x,256x256,512x512}.png` for `bundle.icon`. `tauri-bundler` derives each icon's install path from its own pixel dimensions, so the sizes in the array are the sizes that get installed.
- `src-tauri/icons/tray-{22,24,32,48}.png` from the tray master.
- `frontend/public/favicon.png` at 32 px.

**Every PNG must be RGBA 32-bit with a real alpha channel, not a declared one.** This is the single worst defect in the current asset set and it is worth being precise about, because the current file passes every check you would write casually.

`src-tauri/icons/icon.png` **is** 8-bit RGBA. `tauri-codegen`'s check passes, because that check is `reader.output_color_type().0 != png::ColorType::Rgba` and nothing more. But every alpha value in the file is 255, and the pixels outside the rounded corners are painted opaque near-white `(254,254,254)`. So the corners are not transparent, they are white. On a dark panel the tray renders a white square with a dark tile inside it, and on a light panel it looks fine, which is why this survives casual review.

`assets/icons/echo.png` is the opposite failure. It is colour type 2, plain RGB with no alpha channel at all, which is why adding it to `bundle.icon` would panic the build with `icon ... is not RGBA`.

Both requirements are therefore separate and both are tested in this phase. Every generated PNG carries an alpha channel, **and** the pixels outside the mark are `alpha == 0`. The tray glyph has no tile at all, so its background is fully transparent everywhere, not just at the corners.

**Tooling.** Prefer a Rust binary using `resvg`, run through `cargo run -p xtask`, over a shell script shelling out to `rsvg-convert` or `inkscape`. Neither is installed on the CI image and both would become an apt dependency. `resvg` is a workspace dev-dependency and renders identically everywhere, which matters for the drift check in phase 1's CI.

**`.github/workflows/check.yml`.** Add a drift step: regenerate the icons, then `git diff --exit-code -- src-tauri/icons frontend/public`. This is what stops the sources and the rasters diverging again (**principle-encode-lessons-in-structure**).

**Delete `assets/icons/echo.png`.** It is RGB with no alpha, so it can never enter `bundle.icon` without panicking the build. It is 844 KB for a five-colour flat graphic because 23.5 KB of it is a C2PA provenance manifest naming `gpt-image` as the generator and `trainedAlgorithmicMedia` as the source type. The README calls it "the 1024 source", which is misleading twice over: the real source is the 529-byte SVG, and shipping AI-provenance metadata inside a distributed artifact is a decision to make deliberately rather than inherit (**principle-subtract-before-you-add**).

## Data structures

None. The generator's output manifest is a list of `(master, size, destination)` tuples; keep it as one table in the script so adding a size is a one-line change (**principle-model-the-domain**).

## Verification

**Static.** `cargo test --workspace`. Four assertions per generated PNG, and the alpha ones are the point of the phase:

1. Square, and the declared size matches the filename.
2. `ColorType::Rgba`. This is what `tauri-codegen` enforces with a panic.
3. **All four corner pixels have `alpha == 0`.** Not "some pixel is below 255", which the current `icon.png` would fail but which a slightly different broken file could pass. Corner pixels are exactly where the rounded tile leaves the canvas, so this is the assertion that pins the defect.
4. **For the tray glyph, no pixel outside the bars is opaque.** It has no tile, so a background of any alpha means the master was drawn wrong.

Write these against the current `src-tauri/icons/icon.png` first and watch all of 3 and 4 fail. A test that has never failed has not been tested (**principle-prove-it-works**).

**Runtime.** No control skill covers icon rendering, so this is measured and screenshotted.

1. Rerun the generator on a clean tree and confirm `git diff --exit-code` is clean.
2. Rerun the downsample measurement from the table above against the new tray master at 16, 22, and 24 px and put the numbers in the PR. The target is a full device pixel of gap at 22 px.
3. Render both masters at 16, 22, 24, 32, 48, 128 and 256 px, composite them onto a light and a dark background, and attach the sheet to the PR. The corners must disappear into both backgrounds.
