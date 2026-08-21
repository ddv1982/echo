[Back to overview](overview.md)

# Phase 8: history and dictionary views

## Goal

Restyle the two list views into aura's hairline-table language: border-only row separators, tracked all-caps column labels, tabular numerals for metadata, monochrome hover.

## Changes

- `frontend/src/styles/views.css`:
  - `.table-header` and the History metadata row adopt the tracked label token; the header's tinted `surface-raised` background goes, leaving a hairline bottom rule.
  - `.transcript-row` and `.dictionary-row` hover becomes a one-step lightness shift; row paddings align to a consistent rhythm.
  - `.dictionary-row code` loses its cyan (now grayscale, mono face carries the distinction).
  - `.search-field` inherits the phase 5 input style; keep the leading icon.
  - Empty states drop their lucide illustration icons down to text only, or keep one icon at reduced opacity; pick whichever reads calmer in the screenshot comparison and apply it to both views.
- `frontend/src/App.tsx`. Only if the empty-state decision or metadata layout requires markup changes; otherwise untouched.

## Data structures

None. `HistoryItem` and `DictionaryItem` are unchanged.

## Verification

Static: the three frontend commands.

Runtime: via the control-ui skill, seed history via a fake-engine session, search with matches and without, copy a row and watch the check feedback, add and remove a dictionary entry including the duplicate-removal error path. Screenshots both themes, filled and empty states.
