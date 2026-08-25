# Whisper Small v1.9.2 beam-3 decision

## Identity

- Echo commit: `c973b88d8e6afe20abfdd61f16237f5691ba56d9`
- Echo binary SHA-256: `de916a92a49bcd592d24b8c991f705dda20e8a03fcaad45f179b74e20eb68022`
- whisper.cpp: stable v1.9.2 receipt build
- Runtime SHA-256: `37382797bcad4b4bab155d4a59ac2d41664da19a9f7553c3838052d9efe59199`
- Model: Small multilingual
- Model SHA-256: `1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b`
- Tuning: threads 4, beam 3, best-of 5, temperature fallback enabled
- Intel ICD manifest SHA-256: `09e7ca55461c3f2d65e5df6a6b8f06a7ce2c86fc58a93a18d2dbe3575623de83`
- Device UUID: `8680a6460c0000000002000000000000`
- Driver UUID: `ee99561e45e1e718c6121d36d8345582`
- Pipeline-cache UUID: `35e9eb9761bf7afc9291ffc449ddf849`
- Seed: `20260825`
- Pairs: ten per fixture, twenty FLEURS fixtures, 400 product-CLI transcriptions

## Performance

| Candidate | Median outer ms | p95 outer ms |
| --- | ---: | ---: |
| CPU | 4470.532 | 5697.123 |
| Vulkan | 1860.053 | 2717.989 |

The paired median reduction is 2553.235 ms, or 57.777 percent.

## Quality

| Language | CPU WER | Vulkan WER | Delta pp | Gate |
| --- | ---: | ---: | ---: | --- |
| de | 5.38% | 4.30% | -1.075 | PASS |
| en | 11.36% | 11.36% | 0.000 | PASS |
| es | 4.27% | 4.27% | 0.000 | PASS |
| fr | 11.85% | 11.85% | 0.000 | PASS |
| nl | 13.51% | 13.51% | 0.000 | PASS |

No new silence hallucination occurred. Backend, device, receipt, pairing, sample-size, identity, median, p95, and every quality gate pass.

## Decision

Research result: **PASS**.

Shipping result: **INCOMPLETE**.

The current corpus contains clean read speech only. Project-owned dictation, technical identifiers, fast and quiet speech, noise, false starts, silence, and nonspeech remain missing. The cache cycle has one real boot, so reset evidence also remains incomplete. Production selection stays on managed CPU until both gates pass.

The committed sweep and verifier scripts reproduce this decision. Full raw artifacts remain local under `target/phase5-confirm-small-v192-b3` because the run contains about 2,500 generated files and 15 MB of driver-specific evidence.
