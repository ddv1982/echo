# Phase 3: Capture the inference runtime receipt

[Back to overview](overview.md)

## Goal

Prove the physical device selected by the inference process. Do not rely on
device enumeration from another process. Selected ICD and loaded-runtime
library identity remain launch-evidence responsibilities.

## Changes

- Add a narrow, version-pinned whisper.cpp instrumentation patch that emits one machine-readable receipt after backend creation.
- Add a reproducible build script for the receipt-capable runtime.
- Extend `scripts/probe-whisper-acceleration.py` to parse exactly one receipt and
  bind its selected index to the backend selected in the child loader logs.
- Reject missing, conflicting, software-rendered, or incomplete receipts.

## Data structures

- `RuntimeReceipt`: schema version, backend, selected index, vendor and device IDs,
  API and driver versions, and device, driver, and pipeline-cache UUIDs.
- `ReceiptBuild`: upstream revision, patch digest, build flags, compiler identity, and binary digest.

The runtime receipt deliberately does not claim an ICD manifest digest or loaded
runtime-library digest. Those artifacts belong to the launch evidence.

The inference process emits the JSON after the exact
`echo_whisper_runtime_receipt: ` prefix, so unrelated stderr JSON cannot be
mistaken for a receipt.

## Verification

Static:

```bash
python3 scripts/probe-whisper-acceleration.py --self-test
./scripts/verify-whisper-acceleration.sh
```

Runtime: compare the accelerated receipt with the same-binary `--no-gpu` control on this Linux host. Deliberately select a wrong or software ICD and confirm fail-closed behavior.

## Stop gate

Stop if the selected physical device or loaded runtime cannot be proven from the inference process. Keep the patch out if it cannot remain narrow and reproducible.
