[Back to overview](overview.md)

# Phase 10: motion pass

## Goal

One deliberate motion system instead of scattered per-rule transitions. Minimal design earns its calm through restraint: color and background transitions on interactive elements, a breathing pulse only on the live recording indicators, nothing else moves.

## Changes

- `frontend/src/styles/tokens.css`. Keep the existing duration and easing tokens; they are already sane (120/180/320ms, one ease-out curve). Delete any that end the pass unused.
- `frontend/src/styles/shell.css` and `views.css`. Audit every `transition` and `animation`: interactive elements transition `background-color` and `color` only, at `--duration-fast`; the recording dot and the Home readout keep their pulse; everything else (transform lifts, filter brightness) is already gone from earlier phases, so this phase deletes stragglers and normalizes durations. The `prefers-reduced-motion` overrides in `tokens.css` and `views.css` are consolidated into one place and verified.

## Data structures

None.

## Verification

Static: the three frontend commands, plus `rg 'transition|animation' frontend/src/styles/` and reviewing that every hit matches the system above.

Runtime: via the control-ui skill, exercise hover and recording states; then emulate `prefers-reduced-motion: reduce` and confirm nothing animates. Screen recording of the recording pulse attached to the PR.
