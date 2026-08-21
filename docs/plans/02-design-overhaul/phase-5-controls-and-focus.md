[Back to overview](overview.md)

# Phase 5: controls and focus rings

## Goal

Restyle the shared controls (inputs, primary button, icon buttons, kbd chips) to the flat grayscale language, and replace the glow focus treatment with aura's double-ring focus state on every interactive element.

## Changes

- `frontend/src/styles/base.css`. Inputs go flat: card-surface background, hairline border, no hover border-color tint. Focus becomes the double ring (2px inner ring in the page surface color, 4px outer ring in the accent gray) implemented once via `:focus-visible` with layered `box-shadow` rings, replacing both the current `outline` rule and the `input:focus` glow. This is the one sanctioned use of `box-shadow`.
- `frontend/src/styles/views.css`. `.primary-button` becomes the high-contrast monochrome action: foreground-colored fill with page-colored text in both themes, no border tint, hover as a one-step lightness shift. When recording, it flips to the recording red as today. `.icon-button` loses its border tint hover in favor of background shift; `.danger-button` keeps red hover. `kbd` chips keep the hairline style but drop the doubled bottom border.

## Data structures

None beyond the phase 4 tokens.

## Verification

Static: the three frontend commands.

Runtime: via the control-ui skill, tab through every focusable element on all four views and confirm the double ring renders on both themes, and that button hover and disabled states read clearly. Screenshot focus states.
