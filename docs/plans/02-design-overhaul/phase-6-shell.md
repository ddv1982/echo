[Back to overview](overview.md)

# Phase 6: shell redesign

## Goal

Rebuild the frame around the views: topbar, sidebar, status pill, error banner. Hairline separators, grayscale nav, one theme control for the whole app, and a status pill whose only color is the recording red.

## Changes

- `frontend/src/App.tsx`. Remove `ThemeControl` from the topbar and delete the component; the Settings segmented control becomes the single theme switcher (it already exists and duplicates the topbar control today). Drop the `Waves` brand-mark icon box in favor of a plain wordmark, and drop the "Local dictation" tagline from the topbar (it stays on Home). Update `App.test.tsx` where these elements were queried.
- `frontend/src/styles/shell.css`. Topbar flattens to a hairline bottom border on the page surface; decide `backdrop-filter` keep-or-drop from the WebKitGTK evidence gathered in phase 2's runtime check. Sidebar loses its tinted background and becomes the page surface with a hairline right border. `.nav-item` active state changes from cyan-tinted fill to the aura pattern, a one-step background lightness shift plus foreground-colored text, no colored border. The `.shortcut-card` label adopts the tracked all-caps label token. `.status-pill` goes grayscale for ready and busy tones (dot lightness distinguishes them) and keeps red only for the recording tone; error tone keeps `--danger`. The error banner flattens to hairline border plus soft red fill.

## Data structures

`navigation` in `App.tsx` keeps its `{id, label, icon}` shape; only styling changes.

## Verification

Static: the three frontend commands (tests updated for the removed topbar control).

Runtime: via the control-ui skill, switch themes from Settings and confirm the whole app follows, including after a restart (localStorage persistence). Trigger an error (stop the backend command or use a dictionary duplicate) and check the banner. Confirm the recording pill turns red while recording and grayscale otherwise. Screenshots both themes.
