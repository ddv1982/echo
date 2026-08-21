# Phase 10: writable settings controls

Back to [overview](overview.md).

## Goal

Settings stops being a status readout. The controls that already have a backing setting become writable, and the primitives every later picker needs exist.

Today `SettingsView` (`frontend/src/App.tsx:425-455`) has one writable control, the theme switcher, and it never crosses the IPC boundary. Seven rows are read-only text.

## Changes

**`frontend/src/App.tsx`.** Make the existing settings writable through phase 9's commands: engine, cleanup mode, HUD, hold key, recording seconds.

Reuse `.segmented-control` (`frontend/src/styles/views.css:405-411`) for the small enums. It is already the writable-control idiom in this exact view, the theme switcher uses it, and it needs zero new CSS (**principle-laziness-protocol**).

**Add `select` styling, because the later pickers need it.** There is a real gap: no `select` selector exists anywhere in `frontend/src/styles/`. `base.css` sets `font: inherit` on `button, input` only (`:25-26`), the bare-element input styling covers `input` only (`:49-58`), and the focus ring covers `button:focus-visible, input:focus` only (`:43-47`). An unstyled `<select>` renders as a raw platform widget in both themes.

Add `select` to those three existing rules rather than writing a `.setting-select` class. Three tokens of new CSS against a duplicate of every input rule is not a close call. A segmented control cannot hold a microphone list, a 30-model catalog, or 100 languages, so this is needed regardless.

**Render the override state from phase 9's `source` field.** When a field's source is `env`, show the control disabled with the variable name beside it. A dropdown that silently does nothing because `ECHO_ENGINE` is set would be worse than no dropdown (**principle-experience-first**).

**Restructure the panels.** Three exist today: Appearance, Shortcut and recording, Local pipeline. Adding microphone, model, and language to "Local pipeline" would make one panel carry six controls. Split into Appearance, Audio, Transcription, Shortcut and recording, Text. Do it now, while the panels are nearly empty, rather than after four phases have each added a row to the wrong place (**principle-redesign-from-first-principles**).

**Do not add a Save button.** Write on change. Every field is independently reversible, the file write is atomic, and a Save button on a preferences pane is a modal state to get wrong.

## Data structures

Two small components rather than one generic settings renderer. `SettingSelect` and `SettingToggle`, each taking a label, a value, options, a change handler, and an optional override source. A table-driven renderer over a schema would be the wrong shape here; there are eight controls and they have genuinely different affordances (**principle-laziness-protocol**).

## Verification

**Static.** `npm run build --prefix frontend`, `npm run lint --prefix frontend`, `npm run test --prefix frontend`.

Tests follow the existing rhythm at `frontend/src/App.test.tsx:14-22`: `render(<App />)`, `await screen.findByRole(...)` to clear the 0 ms initial-fetch timer, then synchronous `fireEvent`. Reach Settings with `getByRole('button', { name: 'Settings' })`.

Give every new control an accessible name. The existing dictionary inputs are found via `getByLabelText` because they sit inside a `<label>` with a `<span>` (`:405-407`). A `<select>` without one leaves `getByRole('combobox')` as the only handle, which breaks as soon as there are two.

Cover: changing a select calls the wrapper and the new value renders back; a field with `source: "env"` renders disabled and names the variable.

**Runtime.** Via **control-ui**.

1. Change the engine in the UI. Confirm `~/.config/echo/config.json` on disk changed.
2. Restart the app. Confirm the choice survived.
3. Confirm the pipeline status rows update immediately, not after 10 seconds.
4. Launch with `ECHO_ENGINE=fake` and confirm the engine control is disabled and names the variable.
5. Tab through the whole view. Every control must be reachable and show the double-ring focus state the design system established.
6. Screenshot at 920x680 in both themes and attach to the PR.
