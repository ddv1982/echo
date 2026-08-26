# Microphone switch regression verification

## Claim

Selecting a specific PipeWire microphone must keep the captured PCM when CPAL reports `DeviceNotAvailable` only during intentional stream shutdown. A real error reported before shutdown must still fail capture.

## Reproduction

The selected microphone was the built-in digital endpoint:

```text
pipewire:alsa_input.pci-0000_00_1f.3-platform-skl_hda_dsp_generic.HiFi__hw_sofhdadsp_6__source
```

The recorder used the fake engine and skipped insertion so only capture could fail:

```sh
ECHO_ENGINE=fake ECHO_SKIP_INJECT=1 ECHO_RECORD_SECONDS=1 ECHO_HUD=0 \
  target/release/echo-desktop rec --once
```

Before the fix:

```text
session Idle
session Recording
session Failed microphone capture failed
```

Temporary diagnostic output identified the discarded error:

```text
microphone record diagnostic: Disconnected("Device disconnected")
```

`pw-record` captured the same PipeWire node for two seconds without an error. Echo also succeeded with a clean system-default config. The failure was specific to Echo stopping a selected CPAL PipeWire stream.

## Treatment

`CaptureStreamState` gives the callback and shutdown one locked state transition. Errors recorded in `Capturing` fail the result. `begin_shutdown()` changes the state to `Stopping` before the stream drops, so a teardown-only callback cannot discard valid PCM.

## Native result

The fixed debug binary completed the USB Meteor and built-in digital microphone paths:

```text
session Idle
session Recording
session Transcribing
session Cleaning
session Injecting
session Idle
```

Both exact device IDs produced the same complete sequence.

## Automated checks

- The new shutdown boundary tests passed.
- `xvfb-run -a cargo test -p echo` passed 183 tests and ignored one download test.
- `cargo clippy -p echo --all-targets -- -D warnings` passed.
- `scripts/verify-settings-ux.sh` passed 17 microphone tests, six setup tests, type checking, and both responsive browser tests.
- Four interrogate reviewers found one shared race in the first fix. The corrected state-machine diff received three no-finding verdicts and one non-blocking test-scope note.

Verdict: `VERIFIED`.
