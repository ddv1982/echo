# Phase 10: Run Whisper on the selected device

[Back to overview](overview.md)

## Goal

Make selecting GPU actually execute on the chosen device, with a proven backend and one CPU recovery on any failure.

## Changes

**`crates/echo/src/stt/whisper_gpu.rs`, new.** Resolve the preference and the pin into an execution plan: enumerate, choose the pinned device or the first non-software one, build a Vulkan plan pinned through the two selector environment variables with a forced-CPU fallback plan, and hand both to the existing `RecoveringWhisperEngine`. When enumeration yields nothing, or the pin does not match, return the CPU plan with a stated reason.

**`crates/echo/src/transcribe.rs`.** Replace the `can_accelerate` rule. It currently requires `runtime.source == Managed && runtime.backend == Cpu`, which no bundled runtime satisfies. The new rule is that the preference reads GPU, the Vulkan runtime component is installed, and no tuning or force-CPU override is present.

**Tuning.** Pin beam 3, best-of 5, temperature fallback enabled as a constant applied to both the accelerated plan and its CPU fallback. This is the configuration with 400 transcriptions of zero WER delta across five languages and a 57.777 percent paired median reduction in `.audit/whisper-phase5-small-v192-b3/decision.md`. Applying it to both legs keeps the fallback from being a quality downgrade.

`validate_accelerated` in `whisper_recovery.rs` is unchanged and still rejects a transcript whose receipt does not match the device that was selected, quarantining that device for 24 hours and running one CPU retry.

## Data structures

None persisted beyond phase 9's pin. The plan is built per request from the live enumeration, so there is no route history to grow and no cache to invalidate.

## Verification

Static:

- `xvfb-run -a cargo test --workspace` passes, including shell-stub coverage of the six accelerator failure modes reused from `whisper_recovery.rs`: crash, non-JSON output, missing receipt, wrong device identifier, CPU evidence in stderr, and hang.
- A test proves a pin that matches no enumerated device yields the CPU plan rather than a different GPU.

Runtime: on the development machine with a Vulkan device, dictate with CPU selected and with GPU selected and compare wall time and transcript. Then run `scripts/verify-whisper-acceleration-modes.py --verify-live`, and force `ECHO_WHISPER_TEST_FAULT=no-devices` to confirm CPU recovery.

## Stop gate

Do not proceed to phase 11 until an accelerated run has produced a transcript whose Vulkan receipt matches the selected device, and until a forced failure has produced a CPU transcript rather than an error.
