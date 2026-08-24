# Whisper acceleration probe

| Candidate | Backend | Median outer ms | p95 outer ms | Runs |
| --- | --- | ---: | ---: | ---: |
| cpu | cpu | 1089.738 | 1183.703 | 10 |
| accelerated | vulkan | 463.602 | 474.384 | 10 |

Paired median speedup: **57.6%**.
Paired median reduction: **630.983 ms**.
Decision: **PROCEED**.

## Gates

- PASS: `backendTruth`
- PASS: `hardwareDevice`
- PASS: `pairedCompleteness`
- PASS: `sampleSize`
- PASS: `transcriptParity`
- PASS: `medianSpeedup`
- PASS: `medianReduction`
- PASS: `p95Improved`

This probe proves backend use, paired latency, and exact transcript parity only. It does not replace the multilingual WER/CER and silence corpus gate.
