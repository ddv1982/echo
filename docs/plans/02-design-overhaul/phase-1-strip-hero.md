[Back to overview](overview.md)

# Phase 1: strip the decorative hero

## Goal

Remove the decoration that the new design will not carry: the orb, its pulse rings, the fake waveform, and the hero's radial gradients. The Home hero becomes copy plus the record button, still styled with today's tokens. This is pure subtraction so every later phase works on a simpler base.

## Changes

- `frontend/src/App.tsx`. Delete the `hero-visual` block in `HomeView` (the `orb`, `orb-ring`, and `mini-wave` elements and their hardcoded bar heights). Delete the `Radio` eyebrow icon. Keep the state copy, the primary button, and the shortcut hint. Drop now-unused lucide imports.
- `frontend/src/styles/views.css`. Delete the `.hero-visual`, `.orb`, `.orb-ring*`, `.mini-wave` rules, the `orb-pulse` and `wave-shift` keyframes, and the radial-gradient backgrounds on `.hero-card` (both idle and `data-recording` variants). Collapse the hero grid to a single column. Remove the media-query rules that existed only to hide the deleted visual.

The fake waveform goes because it displays invented data. A minimal design shows nothing rather than a simulation (the experience-first principle: honest feedback over ornament).

## Data structures

None. Markup and CSS deletion only.

## Verification

Static: `npm run build --prefix frontend`, `npm run test --prefix frontend`, `npm run lint --prefix frontend`.

Runtime: via the control-ui skill, load Home in both themes, confirm the hero shows copy, button, and shortcut hint only, and that toggling recording still switches the copy and button label. Screenshot before and after.
