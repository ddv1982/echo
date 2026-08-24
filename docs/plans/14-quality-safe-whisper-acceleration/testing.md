# Testing

## Admission thresholds

An exact identity passes only when all gates pass:

- At least ten randomized CPU and accelerated pairs per fixture and evidence stratum.
- At least twenty licensed fixtures with every required language and product-speech class.
- CPU reports CPU and the accelerator reports one expected physical backend and device.
- Paired median latency improves by at least 20 percent and 500 ms.
- Accelerated p95 is lower.
- Per-language WER or CER does not regress by more than 0.5 percentage points.
- No new silence hallucination or failure regression occurs.
- Fresh and populated cache evidence is bound to tool-owned operations.
- Reset evidence contains at least two complete runs across distinct boot IDs or an equally strong captured reset mechanism.
- Runtime, libraries, model, VAD, tuning, driver, ICD, and device identity remain exact.

Missing evidence is `INCOMPLETE`, not a pass. A failed metric is `STOP`.

## Static checks

Every phase runs the relevant script self-tests plus:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
npm --prefix src-tauri/ui run check
npm --prefix src-tauri/ui run lint
npm --prefix src-tauri/ui test -- --run
npm --prefix src-tauri/ui run build
```

## Runtime checks

Run `echo-desktop transcribe` through the optimized binary. The runtime test must use the same launch contract as production. Capture the actual backend, device receipt, child output, total latency, and fallback cost.

CPU-only hosts, software Vulkan, missing libraries, malformed receipts, wrong devices, driver changes, ICD changes, crashes, timeouts, and malformed JSON are required negative cases before selection ships.

## Product verification

Before delivery, run the complete product verification suite, `cursor-team-kit:deslop`, `pstack:no-comments`, and a multi-model `pstack:interrogate`. Fix every valid finding. After opening the PR, run the pstack Babysit and Shipping playbooks. Merge only a contiguous verified green run, then repeat the main-branch checks.
