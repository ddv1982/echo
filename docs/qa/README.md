# Echo QA

Manual product verification and ship gates. Markdown is the source of truth; [the HTML report](report/index.html) is a generated review view.

| Artifact | Purpose |
| --- | --- |
| [Phase 14 test plan](phase-14-whisper-acceleration-manual-test-plan.md) | Superseded acceleration scenarios, kept for the sign-off record |
| [QA gates](QA_GATES.md) | Gate history; Gate 14 is closed and superseded |
| [Run reports](runs/) | Command and runtime results |
| [Bug reports](bug-reports/) | Reproducible product defects |

Acceleration no longer has a QA gate of its own. Since v0.13.0 it is a setting:
CPU by default, GPU on the device the user picks. What replaced the evidence
gates is the Advanced readout, which names the device that ran and says why a
requested GPU did not.
