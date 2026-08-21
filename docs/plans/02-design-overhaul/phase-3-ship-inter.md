[Back to overview](overview.md)

# Phase 3: ship Inter for real

## Goal

The token file has claimed Inter since day one without bundling it, so users see a system fallback. Bundle the variable font, turn on aura's OpenType feature set, and make numeric metadata tabular. Typography is scaffold; every later visual phase inherits it (the foundational-thinking principle).

## Changes

- `frontend/package.json`. Add `@fontsource-variable/inter` as a dependency (latest version via npm). It ships WOFF2 into the Vite bundle, satisfying CSP `font-src 'self'`.
- `frontend/src/styles/index.css`. Import the fontsource CSS before the local stylesheets.
- `frontend/src/styles/base.css`. Set `font-feature-settings: "calt", "rlig", "salt", "ss01", "ss02"` on `body`. Keep `font-variant-numeric: tabular-nums` on `code`/`kbd` and extend it to the metadata rows (History timestamps, `inferMs`) so columns stop jittering.

## Data structures

None.

## Verification

Static: the three frontend commands. Confirm the built `frontend/dist` contains the Inter WOFF2 files and no external font URL.

Runtime: via the control-ui skill, verify the computed `font-family` on `body` resolves to Inter Variable (evaluate `getComputedStyle` and `document.fonts.check`), in the Tauri WebKitGTK webview and not only in a browser. Screenshot to confirm rendering.
