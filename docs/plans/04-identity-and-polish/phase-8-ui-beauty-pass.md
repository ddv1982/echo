# Phase 8: the beauty pass

Back to [overview](overview.md).

## Goal

The desktop window stops looking like a settings panel and starts looking like a product. Plan 02 made Echo minimal; this phase makes it designed. It runs last so it styles the complete control set, including the model, language, and download rows from phases 3, 5, and 6.

## Changes

Everything is in `frontend/`, plus one small IPC addition. The plan 02 decisions that are not being relitigated: grayscale surfaces, recording red as the only functional hue, the HSL token convention, plain CSS, no component library. Brand warmth enters through the icon mark and illustration, not through controls.

**Brand lockup.** The topbar's plain-text "Echo" (`frontend/src/App.tsx:161-164`) becomes the phase 1 mark as an inline SVG next to the wordmark. Same art as the favicon and the app icon, so the window, the launcher, and the tray finally agree.

**The record hero.** Home leads with the one thing the app does. The record panel becomes a real hero: a large primary record control with a state ring (idle hairline, recording red pulse, transcribing spinner), elapsed time in tabular numerals (`font-feature-settings "tnum"`), and live level bars while recording. The levels are real when the GUI started the recording: a new `get_recording_level` command reads the same `LevelMeter` atomic phase 7 adds, since the GUI's record button records in-process. When the session came from a compositor shortcut the meter is in another process and the bars stay parked at their idle breathing motion; the status pill already communicates that state. A subtle ambient red glow behind the hero while recording is allowed as the single decorative use of the accent.

**Proof of use.** Under the hero, a stats strip computed from the local history store: words dictated, sessions this week, current day streak. The [Wispr Flow Hub](https://docs.wisprflow.ai/articles/5096240724-navigating-the-wispr-flow-app-desktop-ios-and-android) carries exactly this card, and it costs nothing but a reduce over rows already on disk. No network, no accounts, no gamification copy.

**A setup checklist instead of an attention strip.** The current strip (`App.tsx:301-307`) becomes three items with real completion states: microphone ready, speech engine and model installed, shortcut bound. The first two come from the existing health probe. The third cannot be detected, so it is an honest manual check: "I bound it" dismisses it and the dismissal is stored. The checklist links each item into the right Settings section, and it stays until everything is green. This is the [Wispr onboarding](https://www.growthdives.com/p/how-wispr-nails-onboarding) lesson applied to a local app: the first-run job is getting to one successful dictation.

**History grouped by day.** Today, Yesterday, then date headers, with the existing search preserved. Same grouping as the reference apps, and it makes the stats strip's streak legible.

**Empty states with the mark.** The bare-text empty states get a small illustration drawn from the icon's bars motif. One asset, reused, in the current theme's tertiary text color.

**Settings, final form.** The rows added by phases 3, 5, and 6 get their definitive styling: the model select with its size and multilingual metadata laid out as a real choice rather than a bare `<select>`, the detected-language chip, and the download offer as a proper progress row with a verifying state distinct from downloading. Grouping and spacing get one coherent pass across Appearance, Audio, Transcription, Shortcut, and Text.

**Motion.** Hero state transitions, checklist check-offs, and the download progress row animate; `prefers-reduced-motion` already collapses durations globally and must keep doing so.

## Data structures

One new IPC command, `get_recording_level() -> f32`, reading the shared meter when this process holds it and returning 0 otherwise. Stats are derived in the frontend from the existing `get_history` payload; no backend changes. Everything else is markup and CSS in the four existing stylesheets.

## Verification

**Static.** `npm run build --prefix frontend`, `npm run test --prefix frontend`, `npm run lint --prefix frontend`. Component tests cover the stats derivation (empty history, one session, a streak across days), the checklist completion logic including the manual shortcut dismissal, and the parked-versus-live level bar states.

**Runtime.** Via **control-ui**. Screenshot every view at 920x680 in both themes and attach to the PR. Record in-process and screenshot the hero with live bars. Walk the whole window keyboard-only. Verify the recording glow and animations under WebKitGTK specifically, not just Chromium; plan 02's constraint stands that WebKitGTK is the target and CSS features are verified there, with `backdrop-filter` avoided unless proven.
