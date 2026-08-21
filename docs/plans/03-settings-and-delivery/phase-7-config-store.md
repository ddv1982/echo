# Phase 7: the config store

Back to [overview](overview.md).

## Goal

The type that every later phase writes a field to. This is the scaffold, and it lands before any picker (**principle-foundational-thinking**).

Echo has no persisted configuration at all. Every setting is an environment variable read at its point of use, and the GUI's record button and a compositor-bound `echo-desktop rec --toggle` are frequently different processes that share only the filesystem. A dropdown has nothing to write to and the recorder has nothing to read from. That gap is the whole shape of phases 7 through 18.

## Changes

**`crates/echo-core/src/paths.rs`.** Add `config_dir()` and `config_path()` mirroring `data_dir()` at `:6-20`. The chain is `ECHO_CONFIG_DIR`, then `$XDG_CONFIG_HOME/echo`, then `$HOME/.config/echo`, then `/tmp/echo-config` as the last-resort fallback the existing resolvers already use. Landing at `~/.config/echo/config.json`. Export from `crates/echo-core/src/lib.rs:15` beside the other four path helpers.

Config is deliberately separate from `data_dir()`. Dictionary and history are data the user generates; config is preference the user sets. XDG separates them and so should Echo.

**`crates/echo-core/src/config.rs`, new.** One `Config` struct, load and save, following the dictionary store pattern at `crates/echo-core/src/dictionary.rs:28-69` exactly:

- A private file-wrapper struct, `serde_json`, derives on both.
- `#[serde(default)]` on **every** field, so a partial file written by an older version still loads and a hand-edited file missing a key does not fail.
- `write_atomic` on save (`paths.rs:48-65`).
- On a parse failure, `set_aside_corrupt` (`paths.rs:39-44`) and continue with defaults. Never hard-fail. A user with a broken config file should get a working app and a `.corrupt` file to inspect, matching what the dictionary already does.

**Fields for this phase.** Add only the settings that exist today. Later phases add their own field in their own PR:

`engine`, `whisper_model`, `cleanup`, `hud`, `hold_key`, `record_seconds`.

`microphone` arrives in phase 11, `language` in phase 17.

**Precedence: environment, then config file, then default.** Not the other way round. Every existing test sets `ECHO_*`, and inverting the order would break the suite and remove the debugging escape hatch. Put the precedence in one generic resolver so it is stated once rather than re-derived per field (**principle-model-the-domain**).

**Type the fields, do not store strings** (**principle-type-system-discipline**). `engine` is an enum, not a `String`. Today `ECHO_ENGINE` accepts exactly `whisper`, `parakeet`, and `fake`, and anything else, including `Whisper` or a trailing space, falls into a `_` arm and is silently treated as unset. Parsing at the boundary turns that silent failure into a rejected value with a reason (**principle-boundary-discipline**).

## Data structures

`Config { engine: Option<EngineChoice>, whisper_model: Option<String>, cleanup: Option<CleanupMode>, hud: Option<bool>, hold_key: Option<String>, record_seconds: Option<u32> }`.

`Option` on every field is load-bearing. `None` means "not set, fall through to the default", which is what lets a user clear a setting rather than pin it to whatever the default happened to be on the day they opened Settings.

`EngineChoice` is `Whisper | Parakeet | Fake | Auto`, with `Auto` explicit rather than represented by absence. The current code conflates "unset" with "auto-detect" and the UI has no way to show the difference.

## Verification

**Static.** `cargo test --workspace`. Unit tests against a temp `ECHO_CONFIG_DIR`:

- Round-trip save then load.
- A file missing a field loads with that field `None`.
- A file with an unknown field loads and ignores it, which is the forward-compatibility check.
- A corrupt file yields defaults and leaves a `.corrupt` sibling.
- Precedence: env beats config, config beats default.
- An invalid enum value in the file is rejected with a named reason rather than silently becoming `Auto`.

**Runtime.** Nothing reads this yet, by design. Phase 8 migrates the callers, and this phase is deliberately inert so its own PR is small and its tests are pure. Verify by hand that no behaviour changed: run the full existing test suite and `echo-desktop rec --once` with a fixture, and confirm both are identical to before.
