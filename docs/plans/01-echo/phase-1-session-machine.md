# Phase 1. Session machine

Back to [overview](./overview.md).

## Goal

A tested `echo-core` library that owns one dictation session. No audio, no OS APIs. After this phase we can talk about the product in types instead of in wishful `if` chains.

## Changes

`crates/echo-core/src/session.rs` holds the state machine and the only legal transitions.

`crates/echo-core/src/types.rs` holds the PCM newtype, engine id, and failure reasons.

`crates/echo-core/Cargo.toml` is a normal library crate. The workspace root `Cargo.toml` lists it.

## Data structures

`SessionState` is `Idle | Recording { started } | Transcribing | Cleaning | Injecting | Failed { reason }`.

`FailReason` is `MicPermission | InjectPermission | EngineMissing | NoFocus | EngineError | InjectUnconfirmed`.

`Pcm16kMono` is a newtype around `Vec<i16>`. Other sample rates cannot be constructed.

## Verification

Static. `cargo test -p echo-core` and `cargo clippy -p echo-core -- -D warnings`.

Runtime. Table tests cover every legal transition and every illegal one. Holding a key twice does not nest recordings. A failed inject returns to `Idle` only through an explicit `ack` so the HUD can show the reason. No OS runtime in this phase.
