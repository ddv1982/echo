# Echo host matrix decision

| Mode | Median outer ms | p95 outer ms |
| --- | ---: | ---: |
| CPU | 1399.561 | 1626.493 |
| Accelerated | 645.537 | 819.285 |

Median reduction: **761.251 ms (54.209%)**.

| Language | CPU WER | Accelerated WER | Delta pp | Quality gate |
| --- | ---: | ---: | ---: | --- |
| de | 18.28% | 18.28% | 0.0 | PASS |
| en | 10.23% | 10.23% | 0.0 | PASS |
| es | 5.98% | 5.98% | 0.0 | PASS |
| fr | 33.33% | 33.33% | 0.0 | PASS |
| nl | 33.78% | 43.24% | 9.459 | FAIL |

## Gates

- PASS: `completePairs`
- PASS: `pairIntegrity`
- PASS: `sampleSize`
- PASS: `backendTruth`
- PASS: `identityMatch`
- PASS: `hardwareDevice`
- FAIL: `driverIcdIdentity`
- FAIL: `freshAndPopulatedCacheEvidence`
- FAIL: `resetEvidence`
- PASS: `medianReduction`
- PASS: `medianSpeedup`
- PASS: `p95Improved`
- FAIL: `perLanguageQuality`
- PASS: `noNewHallucinations`
- FAIL: `coverageComplete`

Decision: **STOP**.

This warmed, populated-cache clean-read slice cannot satisfy production coverage for dictation, silence, nonspeech, noise, fast speech, quiet speech, technical identifiers, and false starts. Fresh-cache, reset-repeat, explicit driver/ICD, other-hardware, Turbo, memory, power, and failure-path evidence also remain pending.
