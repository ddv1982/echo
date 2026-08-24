# Phase 3: Capture the inference runtime receipt

[Back to overview](overview.md)

## Goal

Prove which physical device and runtime libraries the inference process used. Do not rely on device enumeration from another process.

## Changes

- Add a narrow, version-pinned whisper.cpp instrumentation patch that emits one machine-readable receipt after backend creation.
- Add a reproducible build script for the receipt-capable runtime.
- Extend `scripts/probe-whisper-acceleration.py` to parse and verify the receipt against loader logs and adjacent library hashes.
- Reject missing, conflicting, software-rendered, or incomplete receipts.

## Data structures

- `RuntimeReceipt`: backend, selected index, vendor and device IDs, device, driver and pipeline-cache UUIDs, versions, ICD digests, and loaded runtime-library digests.
- `ReceiptBuild`: upstream revision, patch digest, build flags, compiler identity, and binary digest.

## Verification

Static:

```bash
python3 scripts/probe-whisper-acceleration.py --self-test
./scripts/verify-whisper-acceleration.sh
```

Runtime: compare the accelerated receipt with the same-binary `--no-gpu` control on this Linux host. Deliberately select a wrong or software ICD and confirm fail-closed behavior.

## Stop gate

Stop if the selected physical device or loaded runtime cannot be proven from the inference process. Keep the patch out if it cannot remain narrow and reproducible.
