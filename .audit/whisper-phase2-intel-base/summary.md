# Echo speech benchmark

| Candidate | Language | WER | Median outer ms | Median RTF | Silence hallucinations | Runs |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| whisper:base-q5_1@threads=4,beam=1,best-of=1,no-fallback | de | 18.28% | 661.5 | 0.053 | 0 | 40 |
| whisper:base-q5_1@threads=4,beam=1,best-of=1,no-fallback | en | 10.23% | 592.4 | 0.059 | 0 | 40 |
| whisper:base-q5_1@threads=4,beam=1,best-of=1,no-fallback | es | 5.98% | 646.0 | 0.054 | 0 | 40 |
| whisper:base-q5_1@threads=4,beam=1,best-of=1,no-fallback | fr | 33.33% | 661.4 | 0.060 | 0 | 40 |
| whisper:base-q5_1@threads=4,beam=1,best-of=1,no-fallback | nl | 43.24% | 649.7 | 0.077 | 0 | 40 |
| whisper:base-q5_1@threads=4,beam=1,best-of=1,no-fallback,cpu-only | de | 18.28% | 1418.6 | 0.119 | 0 | 40 |
| whisper:base-q5_1@threads=4,beam=1,best-of=1,no-fallback,cpu-only | en | 10.23% | 1347.8 | 0.136 | 0 | 40 |
| whisper:base-q5_1@threads=4,beam=1,best-of=1,no-fallback,cpu-only | es | 5.98% | 1407.9 | 0.125 | 0 | 40 |
| whisper:base-q5_1@threads=4,beam=1,best-of=1,no-fallback,cpu-only | fr | 33.33% | 1436.0 | 0.134 | 0 | 40 |
| whisper:base-q5_1@threads=4,beam=1,best-of=1,no-fallback,cpu-only | nl | 33.78% | 1383.2 | 0.174 | 0 | 40 |

RTF is `inferMs / audioMs`. Lower is faster.
