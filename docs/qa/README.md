# Echo QA

Manual product verification and ship gates. Markdown is the source of truth; [the HTML report](report/index.html) is a generated review view.

| Artifact | Purpose |
| --- | --- |
| [Phase 14 test plan](phase-14-whisper-acceleration-manual-test-plan.md) | Runnable acceleration scenarios and stop gates |
| [QA gates](QA_GATES.md) | Current phase checklist |
| [Run reports](runs/) | Command and runtime results |
| [Bug reports](bug-reports/) | Reproducible product defects |

GPU availability is never a pass by itself. Missing corpus, reset, receipt, or exact-identity evidence is `INCOMPLETE`, and production remains on managed CPU.
