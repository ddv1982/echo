# Whisper acceleration probe

| Candidate | Backend | Median outer ms | p95 outer ms | Runs |
| --- | --- | ---: | ---: | ---: |
| cpu | cpu | 956.026 | 956.026 | 1 |
| accelerated | vulkan | 8050.036 | 8050.036 | 1 |

Paired median speedup: **-742.031%**.
Paired median reduction: **-7094.01 ms**.
Decision: **STOP**.

## Gates

- PASS: `backendTruth`
- PASS: `hardwareDevice`
- PASS: `runtimeReceipt`
- PASS: `pairedCompleteness`
- FAIL: `sampleSize`
- PASS: `transcriptParity`
- FAIL: `medianSpeedup`
- FAIL: `medianReduction`
- FAIL: `p95Improved`

This probe proves backend creation, its selected physical-device receipt, paired latency, and exact transcript parity only. The receipt does not prove an ICD manifest or loaded-library digests; launch evidence owns those. It does not replace the multilingual WER/CER and silence corpus gate.
