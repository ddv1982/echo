[Back to overview](overview.md)

# Phase 4: grayscale token system

## Goal

Replace the blue-tinted palette with aura's zero-hue grayscale in both themes, keeping recording red as the single functional accent. Collapse the radius scale. Add the terminal-readout label style as tokens. This is the foundational phase; every surface phase after it is mostly re-pointing rules at these tokens.

## Changes

- `frontend/src/styles/tokens.css`. Rewrite both theme blocks:
  - All surface, border, and text tokens become `0 0% L%` triplets. Dark theme runs near-black (page around 3 to 4 percent lightness, cards one step up, hover one more), light theme runs white page with 96 to 98 percent card steps, per the extracted aura palette (white page, `rgb(250,250,250)` soft surface, `rgb(230,230,230)` borders, `rgb(23,23,23)` text).
  - Delete `--brand`, `--brand-strong`, `--brand-soft`. Interactive emphasis becomes lightness (`--accent` as near-foreground gray for active nav, primary buttons, and focus outer ring), matching aura's `hsl(0 0% 20%)` sidebar-primary pattern.
  - Keep `--recording`/`--recording-soft` (red) and `--danger`. Keep `--success` and `--warning` only for status dots.
  - Collapse `--radius-sm/md/lg/xl` to two steps, 8px and 12px, plus `--radius-pill`.
  - Add `--label-tracking: 0.15em` and set heading tracking to aura's tight value; display weights move from semibold toward 500.
- `frontend/src/styles/base.css`. Point heading weights at the new scale.
- Sweep `frontend/src/styles/shell.css` and `views.css` for now-dangling `--brand*` references and re-point them at the new tokens in the same diff (the migrate-callers-then-delete-legacy-apis principle). Visual refinement of those components waits for phases 6 to 9; this phase only keeps every rule resolving.

## Data structures

The token vocabulary is the data shape of the whole redesign: `surface (page, card, raised, hover) / border (subtle, strong) / text (primary, secondary, tertiary) / accent / recording / danger / status-dot colors / two radii / label-tracking`. Later phases may add nothing to it without updating this file first.

## Verification

Static: the three frontend commands, plus `rg 'brand' frontend/src` returning nothing.

Runtime: via the control-ui skill, walk all four views in both themes. Everything renders grayscale except recording and destructive UI. Check text contrast on secondary and tertiary text against WCAG AA in both themes (spot-check computed colors with a contrast calculator). Screenshots both themes.
