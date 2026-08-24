# Phase 2. Introduce the execution plan

Back to [overview](overview.md).

## Goal

Give runtime, backend, tuning, model, VAD, and protocol one owner without changing current behavior.

## Changes

- `crates/echo/src/stt/whisper_plan.rs` introduces the execution plan, runtime candidate, tuning, protocol, and worker key types. Its first policy resolves managed CPU, one-shot CLI, and upstream defaults.
- `crates/echo/src/stt/runtime.rs` returns typed managed and system runtime candidates instead of collapsing discovery to one binary before policy runs. Existing healthy managed CPU precedence remains the initial policy.
- `crates/echo/src/transcribe.rs` resolves and leases one plan before building `WhisperEngine`.

No persistent config field or Settings control is added. Manual models and compatible system binaries remain valid candidates.

## Data structures

- `WhisperExecutionPlan`. Runtime candidate, model, VAD, tuning, and protocol.
- `WhisperRuntimeCandidate`. Source, backend, CLI, optional server, probe report, and managed provenance.
- `WhisperTuning`. Threads, beam size, best-of, and fallback policy.

## Verification

Static:

- Resolver tables cover managed CPU, system CPU, missing server, manual model, managed corruption, and source precedence.
- Existing transcription and runtime tests remain unchanged where behavior is intentionally identical.

Runtime:

- Run the shipping CLI with managed runtime plus managed model, system runtime plus managed model, managed runtime plus manual model, and system runtime plus manual model.
- Confirm every result records the selected candidate and exact artifact paths.
