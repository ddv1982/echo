# Whisper acceleration probe

| Candidate | Backend | Median outer ms | p95 outer ms | Runs |
| --- | --- | ---: | ---: | ---: |
| cpu | cpu | 1014.901 | 1014.901 | 1 |
| accelerated | vulkan | 421.821 | 421.821 | 1 |

Paired median speedup: **58.437%**.
Paired median reduction: **593.08 ms**.
Decision: **STOP**.

## Gates

- PASS: `backendTruth`
- PASS: `hardwareDevice`
- PASS: `runtimeReceipt`
- PASS: `pairedCompleteness`
- FAIL: `sampleSize`
- PASS: `transcriptParity`
- PASS: `medianSpeedup`
- PASS: `medianReduction`
- PASS: `p95Improved`

This probe proves backend creation, its selected physical-device receipt, paired latency, and exact transcript parity only. The receipt does not prove an ICD manifest or loaded-library digests; launch evidence owns those. It does not replace the multilingual WER/CER and silence corpus gate.
