[Back to overview](overview.md)

# Phase 7: home view redesign

## Goal

Make Home the calm core loop: record, see what you said, get out. One primary surface with the record affordance and honest live feedback, setup status only when something needs attention, and the transcript panels underneath.

## Changes

- `frontend/src/App.tsx`, `HomeView`:
  - The hero becomes a flat record panel. The state copy stays, restyled with the phase-label as a tracked all-caps readout ("READY", "LISTENING", "TRANSCRIBING"). While recording, show elapsed seconds against `status.maxRecordSeconds` (a client-side timer keyed on the `recording` flag; the backend exposes no start timestamp and does not need to).
  - Replace the four-card `health-grid` with progressive disclosure. When everything is ready, show nothing (the always-ok "Suggested shortcut" card is filler and goes entirely). When a component is not ready, show one compact attention strip naming it, linking to Settings. This removes the duplication with the Settings pipeline section (the subtract-before-you-add principle).
  - `HealthCard` shrinks or folds into the attention strip; keep whichever shape leaves less code.
  - Last transcript and Recent panels stay, with `SectionHeading` losing its icon chip (title plus subtitle only, tracked label style). This changes `SectionHeading` for all callers, including Settings, which phase 9 restyles.
  - Update `App.test.tsx` for the removed health grid and changed headings.
- `frontend/src/styles/views.css`. New record-panel rules; delete `health-grid`/`health-card` rules if the strip replaces them; `section-heading` drops the `.section-icon` box.

## Data structures

`AppStatus` (in `frontend/src/types.ts`) is unchanged; the attention strip derives from the existing `microphoneReady`, `engineReady`, `injectionReady` booleans. The elapsed timer is local component state, one number.

## Verification

Static: the three frontend commands.

Runtime: via the control-ui skill, with the fake engine (`ECHO_ENGINE=fake`) record a session end to end and watch the readout move READY to LISTENING with a counting timer to TRANSCRIBING and back, and the transcript land in Last transcript and Recent. Break one component (unset the engine) and confirm the attention strip appears and routes to Settings. Screenshots both themes, idle and recording.
