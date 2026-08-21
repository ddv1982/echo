[Back to overview](overview.md)

# Phase 2: strip global decoration

## Goal

Make the app flat. Remove every box shadow, glow, texture gradient, and inset highlight so depth comes only from surface lightness and hairline borders, matching aura's "no box-shadow on components" rule.

## Changes

- `frontend/src/styles/tokens.css`. Delete `--texture`, `--shadow-card`, and `--shadow-raised` from both themes.
- `frontend/src/styles/shell.css`. Remove the texture layer from `.app-shell`, the inset highlight on `.brand-mark`, and the `box-shadow` halo on `.status-dot` (keep the dot itself). Keep the topbar `backdrop-filter` for now; phase 6 decides its fate on WebKitGTK evidence.
- `frontend/src/styles/views.css`. Remove `box-shadow` from the `.panel`/`.hero-card`/`.health-card` shared rule, the glow and inset shadow on `.primary-button`, and the `translateY` hover lift. Hover feedback becomes a background lightness shift only.

## Data structures

None. Token and rule deletion only.

## Verification

Static: the three frontend commands, plus `rg 'box-shadow|--texture|--shadow-' frontend/src/styles/` returning only the focus-ring rules that phase 5 will replace.

Runtime: via the control-ui skill, walk all four views in both themes and confirm panels separate by border and background alone. Screenshot before and after.
