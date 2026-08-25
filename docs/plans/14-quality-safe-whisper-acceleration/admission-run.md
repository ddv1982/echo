# GPU admission continuation

## Definition of done

GPU acceleration is finished only when one exact current identity satisfies every admission gate, production selects only that identity, one accelerated failure triggers at most one same-model managed-CPU retry, Linux packages contain the qualified runtime contract, merged `main` is green, and a tagged release is published and verified.

An available Vulkan device, a successful smoke test, or an older performance result is not completion.

## Scope and rigor

This is a high-rigor run because a false pass changes user transcription quality and availability. The candidate is stable whisper.cpp v1.9.2, multilingual Small, four threads, beam size 3, best-of 5, and temperature fallback enabled on the observed Intel Iris Xe identity.

The current clean corpus has twenty fixtures. Completing the product matrix requires at least one defensible fixture for each of eight required classes. A 28-fixture, ten-pair confirmation is 560 measured `echo-desktop` transcriptions plus warmups. At the historical medians, the measured child time alone is roughly thirty minutes before hashing, replay, and receipt checks.

The reset gate requires two hardened cycles from distinct Linux boot IDs. The first can run now. The second requires a real reboot and cannot be simulated with labels.

## Data shape

`CorpusFixture` is the unit of quality evidence. It binds one audio digest to one reference, language, product class, source license, and derivation record. Derived stress audio may cover acoustic conditions such as speed, level, and noise only when the source and transform are pinned. It cannot be relabelled as spontaneous dictation, technical identifiers, or false starts unless the spoken content actually has that property.

`AdmissionIdentity` binds the Echo binary, runtime CLI and adjacent libraries, model, VAD, pinned-language and empty-prompt policy, decoding, device receipt, ICD manifest and library, Mesa cache class, and evidence digests. Boot ID separates reset strata but is not a production wildcard.

## Workflow

1. Inventory every local input and make missing coverage explicit.
2. Capture hardened cycle A on the current boot. Verify the composite runtime, driver, cache snapshots, receipt, and transcript parity.
3. Build the smallest licensed product corpus with deterministic fetching or derivation. Add a verifier that rejects missing attribution, digest drift, transform drift, and class relabelling.
4. Stop for the throughput checkpoint. Review cycle A and corpus coverage before starting the long qualification.
5. Run the exact current product launcher for ten randomized CPU/Vulkan pairs per fixture. Replay every warmup and measurement. Record `VERIFIED`, `NOT VERIFIED`, or `INCONCLUSIVE`.
6. After a real reboot, capture cycle B with cycle A as prior evidence. Require distinct boot IDs and matching exact identity.
7. Recompute the admission decision. If any gate fails, tune or stop. Do not implement selection.
8. If every gate passes, implement exact selection, bounded quarantine, and one managed-CPU logical retry.
9. Package the admitted runtime without broadening the identity. Evaluate warmup or residency only after one-shot selection passes.
10. Run full product verification, adversarial review, PR babysitting, merge verification, then tag and verify a release.

## Stop gates

- Stop corpus work if licensing, attribution, spoken reference, or class assignment is not auditable.
- Stop qualification if CPU and Vulkan differ in anything except GPU enablement.
- Stop admission if current-launcher quality, latency, p95, receipt, cache, reset, or identity evidence is missing or failed.
- Stop selection if an unpassed identity can run, fallback changes the contract, or more than one managed-CPU logical retry is possible.
- Stop release if merged `main`, packages, published assets, or the installed product path is not green.

## Throughput checkpoint

The checkpoint is after cycle A and complete corpus verification. Before the long run, report the exact fixture count, observation count, estimated duration, disk budget, boot identity, runtime identity, and any remaining manual boundary.
