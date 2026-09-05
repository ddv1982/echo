# Recording coordination implementation

The implementation follows [Plan 21](../21-recording-coordination.md). The remaining acceptance work is real-desktop and elapsed routine-use verification, described below. It is not replaced by the automated checks.

## Implemented behavior

`PublishedSession` in `echo::rec` owns the existing `Session`, recording lease, and publication destination. The run body calls its transitions rather than independently mutating phase and deciding whether to publish. `Injecting` is published before the insertion effect is invoked. Successful terminal publication follows the History append and exposes an ID only if the append succeeded.

A publication failure before capture, transcription, or insertion stops the next effect. If insertion was already attempted, its transcript still reaches the existing History append path even when publishing the failed phase also fails. A dropped worker's scoped active status is valid only while its matching lease is live. This clears stale active state when the desktop process stays alive after the worker exits. Failed terminal states remain visible, and legacy unscoped records retain process-identity recovery.

`ControlIntent` concentrates explicit stop/cancel phase eligibility and token-checked intent delivery. `RecordingControlAck::after_revision` is the shared receipt projection rule. Owner publications still reserve the intervening revision, and the wire contract is unchanged. The legacy toggle decision remains based on the observation made before capture-stop is written.

The frontend's `recordingObservation` policy owns poll-epoch, reply-session, and same-session revision acceptance. `useAppController` holds one observation ref instead of two separate refs and retains its History/Dictionary effects. The epoch and snapshot values still exist because they solve different ordering problems. The synchronous request guard and render-visible pending state remain.

| Previous policy distribution | Current implementation |
| --- | --- |
| Separate phase mutation and status-write sequences in the recording run | Transition and publication methods in `PublishedSession` |
| Two explicit control receipt factories | One `request_control_ack` path parameterized by `ControlIntent` |
| Separate low-level stop and cancel token-validation implementations | One token-scoped intent writer |
| Receipt arithmetic in command handling and desktop projection | One receipt-revision rule |
| Frontend ordering predicates in multiple app-controller handlers | One recording-observation policy with a typed observation origin |

This concentrates policy; it does not claim fewer total lines. Maintained tests, native verification, and failure handling add code. No new daemon, transport, runtime dependency, or independent phase machine was introduced.

## Maintained verification

The real CLI tests use `CARGO_BIN_EXE_echo-desktop`, synthetic fixture audio, isolated data/config/model directories, and a gated fake speech engine. They cover ordinary stop with transcript/History preservation, intentional cancellation, restart, owner death during capture and transcription, stale legacy token intents, and real History append failure followed by recovery. Existing library tests retain scoped-intent coverage.

The publication tests block the insertion effect while another thread reads the actual status file. A separate filesystem fault prevents publication, proves the effect was not invoked, restores the old active status, and proves lease loss clears it while the same PID stays alive. A replacement owner then successfully publishes.

Training tests preserve the existing blocked-start cancellation proof and add duplicate-start, foreign-ID, and one-time active-resource release cases.

The native probe calls the production Tauri adapter from a real WebKit process. It verifies ten recording/configuration contracts, measures receipt and observation timing, and runs the existing status IPC probe. It records source identity, binary hash, build profile, and environment. A missing verification payload, failed assertion, wrong binary commit, or source mutation invalidates the result. The fixture does not use a real microphone or inject text into another application.

CI now runs the native probe, preserves its JSON artifact, and explicitly runs isolated X11 registration and private-bus portal routing checks. The portal fixture now isolates configuration and model paths as well as recording data. It previously consulted host state and timed out in local verification.

Representative commands from the repository root:

```sh
cargo test -p echo-desktop --test cli_rec
cargo test -p echo --test recording_commands
cargo test -p echo --lib injecting_
cargo test -p echo --lib status::tests::
cargo test -p echo-desktop --bin echo-desktop commands::dictionary_training::tests::
python3 scripts/verify-recording-native.py --output /tmp/echo-native-recording.json
```

Use `--release` on the native probe for an optimized measurement. The probe builds below `target/recording-native-probe`, preserving the normal debug executable. Set `CARGO_TARGET_DIR` to select the parent target directory. If another command may build into that directory, coordinate the entire build/run with the probe's `--lock-file` or `ECHO_COORDINATION_LOCK_FILE`. Do not share a target with deliberately mutated source tests.

## Transport decision

Retain the scoped file controls and serial frontend polling. The [baseline](baseline.json) is a pre-refactor debug probe with verification instrumentation. It observed successful controls and approximately timer-resolution status IPC; it is not a production latency guarantee. The treatment evidence is in [verification.json](verification.json).

The sample does not establish a failure rate or a statistically significant latency change. It also measures idle status projection after the synthetic run, not every active-phase or real-engine workload. There is no demonstrated transport problem here that justifies endpoint discovery, another responsive control task, protocol negotiation, and reconnect/replay behavior. Phase 5 therefore ends with retaining the simpler transport. Reopen that decision only with a reproducible remaining problem and a concrete deletion budget.

## Remaining product acceptance

The automated lanes do not prove physical microphone behavior, compositor permissions, insertion into real target applications, or one to two weeks of routine use. The private-bus portal test uses a test portal; it is not a claim that every Wayland compositor passed. X11 grab registration under Xvfb does not exercise physical keyboard delivery.

Run the following on the actual supported desktop before signing off that lane. Use disposable text in a disposable target document. Do not collect transcripts or clipboard contents in the evidence log.

1. Record the source/release version, desktop/session backend, engine, and test date.
2. Start and stop through Home, then through the configured shortcut and tray. Confirm a single transcript, a recoverable History result, and correct visible progress.
3. Issue a distinct cancel during processing where that adapter supports it. Confirm it does not insert text or claim a successful History result.
4. Repeat recording after success, cancellation, and a recoverable device/engine failure.
5. Exercise training alongside a dictation attempt and confirm only one capture owns the lease.
6. Check focus/visibility changes and a supported deferred upgrade while processing. Confirm no automatic reinjection.
7. Record ordinary-use failures for one to two weeks using categories and counts rather than text content.

Suggested record fields are date, version, backend, action, expected outcome, observed outcome, whether retry recovered, and a count of affected runs. Report the number of observations. Zero failures in a small set is not a reliability percentage.

Any wrong-session action, duplicate insertion, transcript loss after an ordinary successful stop, or unrecoverable active state blocks sign-off for that changed lane. The current code does not promise recovery of audio or a transcript after a process crash before History append. Durable pre-insertion journaling remains a separate storage/privacy decision.

## Review and evidence

The [decision trail](decisions.tsv) records design choices and verification boundaries. Independent code review found no blocking issue in the owner/publication, lease-aware recovery, control-receipt, or frontend-ordering changes. A documentation finding required replacing the old research-only status with this implementation record.

The local full suite passed 525 Rust tests with 16 environment/helper cases ignored, and 275 frontend tests. The isolated X11 and portal checks were run separately. One responsive-browser run timed out during concurrent Rust compilation; its unchanged targeted rerun passed. The complete final browser/native results and source identity are recorded with the PR verification evidence.
