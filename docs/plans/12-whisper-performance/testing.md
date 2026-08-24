# Testing

Back to [overview](overview.md).

## Measurement contract

Every raw observation records:

- Echo commit and version.
- Host OS, kernel, CPU, GPU, driver, memory, power mode, and relevant environment.
- Binary, server, model, VAD, and managed generation identities and SHA-256 values.
- Requested and resolved backend, runtime source, protocol, threads, beam, best-of, fallback, VAD, language, and prompt length.
- Cold, resident-first, resident-warm, or cold-fallback mode.
- Outer wall, audio encoding, queue, process start, model load when available, inference, parsing, attempts, and total time.
- Transcript, reference, WER or CER, language, silence hallucination, and failure.

Do not combine cold and warm observations. Do not compare different hardware in one ranking. Publish raw JSONL beside every summary.

## Corpus

Use at least twenty project-owned or appropriately licensed dictation fixtures. Cover Dutch, English, German, French, and Spanish first, then every language used to justify a runtime policy. Include normal speech, fast speech, technical identifiers, short commands, long paragraphs, quiet input, noise, false starts, silence, and nonspeech audio.

The two committed fixtures remain contract smoke tests only. They cannot decide performance or quality.

## Experiment rules

- Build Release once per commit and record its binary hash.
- Hold model, corpus, VAD, language, prompt, and backend constant while testing one tuning dimension.
- Randomize candidate order inside each fixture block with a stored seed.
- Document warmups and run at least ten timed observations per fixture.
- Keep power policy and background load stable.
- Report median and p95 user-path latency, RTF, WER or CER, silence hallucinations, failures, and RSS.
- Repeat accepted candidates after reboot or a clean device state.

## Quality gates

- No new silence hallucination.
- No per-language WER or CER regression above 0.5 absolute percentage points.
- No English aggregate may hide a regression in another required language.
- No performance fallback may change model.
- No candidate with a failed or retried request may be counted as a successful latency sample without its attempt cost.

## Lifecycle matrix

Cover:

- Simultaneous broker startup from separate processes.
- Client exit before ready and during inference.
- Broker exit before state write, after state write, and while idle.
- Server crash during load and inference.
- Cancellation with confirmed clean idle and uncertain cleanup.
- Model, runtime, VAD, tuning, backend, driver, and managed generation changes.
- Managed repair or removal while a worker holds leases.
- Stale PID, stale endpoint, malformed state, permission failure, and occupied port.
- Idle shutdown and immediate restart.
- Accelerated system failure followed by managed CPU fallback.

## Static verification

Run the project-level commands from [overview.md](overview.md). Keep Fake-driven contract checks in normal CI. Keep real model, GPU, resident, and long-running performance matrices opt-in and publish their raw artifacts on the PR.

## Runtime verification

- CLI. Drive `echo-desktop transcribe` with real files for every cold candidate and compare JSON with raw report rows.
- Recording. Use `ECHO_AUDIO_FIXTURE` to exercise `rec --once` and cross-process `rec --toggle` without opening a real microphone.
- UI. Drive the native Settings view after cold, warm, accelerated, and fallback runs and inspect its actual values and responsive layout.
- Installer. Install, verify, repair, remove, interrupt, and resume each managed runtime component on target Linux hardware.
- Release. Build and smoke-test the same Debian, RPM, binary, and AppImage artifacts used by GitHub Actions.

No phase is complete from unit tests alone. Its matching runtime path must pass.
