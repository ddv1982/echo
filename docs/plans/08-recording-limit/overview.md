# Recording limit

## Problem

Echo v0.7 has one stored duration but two policies. `rec --once` resolves `ECHO_RECORD_SECONDS` over `record_seconds` over a three-second default. Toggle recording ignores that value and always stops at a fixed 60 seconds. Tauri and browser preview repeat those numbers, and Settings describes the field as a timed CLI option.

The 60-second value entered the project as a missed-toggle watchdog. Git history contains no engine, memory, accuracy, or UX evidence for that exact number. Raising it without changing audio conversion would be careless. Ten minutes of 48 kHz stereo input is about 230 MB of native f32 data, and the current conversion clones it and allocates a full mono buffer before creating the 16 kHz output.

## User contract

- Every recording route uses one maximum recording length.
- The default and hard ceiling are ten minutes.
- Toggle recording can stop sooner on the second press.
- `rec --once` records until the same limit.
- `ECHO_RECORD_SECONDS` still overrides the saved value.
- Existing `record_seconds` JSON stays valid. Values from 61 through 600 stop being truncated.
- Settings offers 30 seconds, 1 minute, 2 minutes, 5 minutes, and 10 minutes.
- Home shows the limit snapped by the process that owns the active recording.

## Shape

`echo-core::recording` owns `RecordingLimit`, the 1..600 range, the 600-second default, preset values, source attribution, environment parsing, and precedence. `Config.record_seconds: Option<u32>` remains unchanged. Raw environment and file numbers become a valid `RecordingLimit` at the boundary. Recorder code accepts the type and does not clamp again.

The recorder resolves the limit once after it owns the recording session. Timed and toggle paths pass that same value to one capture deadline. A toggle watcher can cancel earlier but cannot choose a different maximum.

The status file gains an optional `recording_limit_seconds` line for Recording. Tauri uses that active snapshot while recording and the current resolved setting while idle. Old status files remain readable. A start timestamp is deliberately out of scope; the current local elapsed display remains.

Settings gains policy metadata from Rust. The Maximum recording length row moves to General. Choosing the ten-minute default clears the saved override. Non-preset old values remain visible, and environment-backed values remain locked.

Shortcut verification calls a stop-only backend command after it sees the expected activation. The command can stop a live recording but can never start one. This keeps a successful test from leaving a ten-minute capture running.

## Capture memory

The capture callback still records native interleaved f32 samples. After the stream stops, `AudioCapture::record` moves the vector out of the mutex instead of cloning it. Resampling averages source frames on demand and writes directly into the final 16 kHz i16 output instead of allocating a full mono f32 vector.

At 48 kHz stereo for ten minutes, logical sample payload falls from about 595 MB to about 250 MB. Echo keeps the proven batch resampler and does not pre-reserve the full ten-minute vector. Short recordings stay cheap.

## Synthesis

Candidate SOL is the base. The cross-judge scored it 23/25. Grafts are a dedicated `status::write_recording(limit)` helper from Review-SOL, the explicit no-start-timestamp and preview-snapshot boundaries from Planck, and checked sample-budget arithmetic tests from Mendel.

Rejected work includes a streaming resampler, eager ten-minute allocation, new config schema, free-form duration input, recording start-time protocol, replaying toggle for shortcut cleanup, and workflow restructuring. These add more risk than the requested setting earns.

## Implementation order

1. Remove the native clone and intermediate mono allocation while the 60-second behavior stays unchanged.
2. Add the typed policy and route timed and toggle capture through one snapped limit.
3. Add active-limit status, Tauri projection, General Settings control, preview behavior, and shortcut cleanup.
4. Add the deterministic verifier, docs, v0.8.0 identity, adversarial review, PR, merge, tag, and release proof.
