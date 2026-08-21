# Phase 8: migrate the environment readers

Back to [overview](overview.md).

## Goal

Every setting reads from one place. Delete the scattered direct environment reads in the same diff (**principle-migrate-callers-then-delete-legacy-apis**).

The payload of this phase is not "config now works". It is collapsing four hand-synced pairs, each of which is a place where the UI can already tell the user one thing while the recorder does another.

## Changes

**Collapse the duplicated decisions first, then point the survivor at `Config`.** Doing it in that order keeps each step reviewable.

| Pair | Locations | Problem |
| --- | --- | --- |
| `resolve_engine` / `engine_summary` | `crates/echo/src/stt/mod.rs:19-36`, `:42-59` | Each independently branches on `ECHO_ENGINE` and each independently implements the Parakeet-then-Whisper fallback. The doc comment at `:38-40` says they must mirror each other, by hand. |
| `from_env` / `mode_name` | `crates/echo/src/cleanup/mod.rs:15-24`, `:34-41` | Same shape. |
| `hud_disabled` / `enabled` | `crates/echo/src/ui/hud.rs:71-76`, `:81-83` | Same shape. |
| the two `default_input_device` lookups | `crates/echo/src/audio.rs:81-82`, `:89-90` | Phase 11 owns this one. Named here because it is the same class of bug. |

For each pair, keep one function that resolves the decision and returns a value carrying enough information for both callers. The status label becomes a projection of the resolved decision rather than a parallel re-derivation.

**Then migrate the six readers** to consult `Config` with the phase 7 precedence: `resolve_engine`, cleanup `from_env`, `hud_disabled`, `hold_key` (`crates/echo/src/hotkey.rs:114-119`), `recording_duration` (`crates/echo/src/rec.rs:313-322`), and the Whisper model name.

**Do not expose `ECHO_SKIP_INJECT`.** It exists for tests. Adding it to the config file would put "don't actually type anything" one click away from a user who does not know what it means (**principle-experience-first**).

**Delete every direct `std::env::var` call the migration replaces.** Leaving one behind means a setting that works in one code path and not another, which is the exact failure this phase exists to remove.

## Data structures

One resolved-settings value threaded from a single load point, rather than a config handle passed around. `run_record` (`crates/echo/src/rec.rs:81`) and `get_app_status` (`src-tauri/src/main.rs:97-115`) already call every one of the six functions, so a load at the top of each entry point reaches everything with no new plumbing.

Load once per process, not per call. A recording session must not change engine halfway through because the user saved Settings mid-utterance (**principle-separate-before-serializing-shared-state**).

## Verification

**Static.** `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`. The existing suite is the primary check: 51 tests pass today and they set `ECHO_*` throughout, so an intact suite proves the precedence order is right.

Add one test per collapsed pair asserting the resolver and its label agree, which is the invariant the doc comments have been asking a human to maintain.

**Runtime.** Both process paths, because they are not the same code path and this is where that bites.

Via **control-cli**: write a config file with `engine = "fake"`, run `echo-desktop rec --once` with a fixture in a **clean environment with no `ECHO_*` set**, and confirm the fake engine ran. Then set `ECHO_ENGINE=whisper` and confirm the environment wins.

Via **control-ui**: open the app, confirm the Settings rows reflect the config file rather than the defaults, and confirm the reported engine matches what a recording actually uses.

Then the path that is easy to skip. Bind `echo-desktop rec --toggle` to a GNOME shortcut, press it, and confirm the config-file setting applied. A setting that works through the GUI button and fails through the bound shortcut looks fine in every test you would think to write.
