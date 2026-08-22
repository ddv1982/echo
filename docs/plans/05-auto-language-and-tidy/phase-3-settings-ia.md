# Phase 3: settings for humans

Back to [overview](overview.md).

## Goal

Settings a normal user can finish. Two tiers: a short General surface with the four decisions that matter, and an Advanced disclosure for everything else. Same grayscale design language; this is information architecture and copy, not a re-theme.

## Changes

**`frontend/src/App.tsx`, the Settings view restructured.** The General surface, in order:

- **Microphone.** Unchanged control, plain label. The Test button stays.
- **Language.** The phase-1 control: Auto first, the detected-language chip, and the pin suggestion. The copy stops saying "Pin a language, or let Whisper detect it" and says what Auto does in plain words.
- **Model quality.** The model select and the download offers merge into one choice presented as quality tiers (Fast, Balanced, Best) with size and language capability in plain text. Filenames and URLs move to the Advanced tier's transparency readout. The download progress row is unchanged.
- **Theme.** Unchanged.

**Advanced, behind a collapsed-by-default disclosure.** Engine override (Auto/Whisper/Parakeet, with the unavailability reasons), the transparency readout (resolved engine, last run, model file, binary, multilingual, VAD, version), hold key, timed recording, cleanup mode, HUD toggle, the env-override readout, and the config file path. The env-var names stop appearing as hint text on the General surface; they live in Advanced where the audience wants them.

**Copy pass.** No `rec --hold`, `rec --once`, "VAD", or "X11 sessions" in user-facing labels. "Hold key" becomes "Push-to-talk key". "Timed recording" becomes "Recording length for timed sessions" or moves wholly to Advanced. The suggested shortcut stops appearing twice; the sidebar card stays, the Settings row goes.

**What does not change.** The token system, the component patterns (`setting-row`, `segmented-control`, `SettingSelect`), both themes, and every behavior. The phase moves and renames controls; it does not add capabilities. The language warning, the stale-install card, and the download machinery keep working from their new positions.

## Data structures

None. The IPC surface is unchanged; this phase is markup and copy.

## Verification

**Static.** `npm run build/test/lint --prefix frontend`. Component tests pin the tiering: General shows Microphone, Language, Model quality, Theme and nothing else; Advanced is collapsed by default and reveals the engine override on expand; the Fake engine is absent everywhere (phase 2); env-locked fields still render locked in Advanced.

**Runtime.** Via **control-ui**. Screenshot both tiers at 920x680 in both themes and attach to the PR. Walk the whole view keyboard-only: the disclosure expands from the keyboard and every control is reachable.
