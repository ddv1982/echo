# Phase 8. Dictionary

Back to [overview](./overview.md).

## Goal

A stored list of phrases that rewrites a transcript after STT and before inject. "clawed code" becomes "Claude Code", the way the video's dictionary did. This pass runs on every engine so Linux is not a second-class citizen.

## Changes

`crates/echo-core/src/dictionary.rs` is the store and the rewriter.

`crates/echo/src/ui/dictionary.rs` is a later-window concern. In this phase a `echo dict add "Claude Code"` CLI is enough. The GUI list waits for phase 9.

Persistence is a JSON or TOML file in the platform data dir. One file. No sqlite.

## Data structures

`DictEntry` is `{ spoken: String, written: String, created_at }`.

`Rewrite` is `{ text: String, hits: Vec<DictHit> }`. `DictHit` is `{ entry, span }`. The HUD can say Corrected when `hits` is non-empty, matching the video.

Matching is case-insensitive and whole-phrase. Do not regex user entries.

## Verification

Static. `cargo test -p echo-core` covers rewrite hits, misses, overlapping phrases (longest wins), and empty input.

Runtime. Transcribe the `claude_code.wav` fixture with the entry present and assert the injected text contains `Claude Code`. Repeat with the entry absent and assert it does not. Engine errors are out of scope. This is a string pass.
