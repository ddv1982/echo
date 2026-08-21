[Back to overview](overview.md)

# Phase 9: settings view

## Goal

Settings becomes the single, calm system-of-record: definition-list rows with hairline separators, tracked section labels, and status communicated by small dots plus text instead of tinted badges.

## Changes

- `frontend/src/App.tsx`, `SettingsView`:
  - Section headings use the iconless `SectionHeading` from phase 7.
  - `SettingLine` badges (`Ready`, `Setup`, `Check`, `On`, `Off`) become a small status dot (success, warning, or neutral gray) beside plain text, reserving fills for the recording state only. The `badge` prop becomes a status tone.
  - The theme segmented control (now the app's only theme switcher, per phase 6) restyles to the monochrome active treatment.
- `frontend/src/styles/views.css`. `.settings-section`, `.setting-row`, `.setting-line` move to the hairline rhythm; `.small-badge` is deleted; a `.status-dot`-style indicator is shared with the shell's pill rather than duplicated (one source of truth, the laziness-protocol principle).

## Data structures

`SettingLine`'s prop shape changes from `badge?: string` to a small closed union of status tones. One-line type in `App.tsx` or `types.ts`.

## Verification

Static: the three frontend commands.

Runtime: via the control-ui skill, view Settings with a ready pipeline and with a missing engine, confirm dots and text change accordingly, and switch all three theme modes from here. Screenshots both themes.
