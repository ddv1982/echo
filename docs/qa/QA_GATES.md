# QA gates

**Last QA merge:** [2026-08-25 Phase 14 follow-up](runs/COORDINATOR-MERGE-2026-08-25-2.md)

## Gate 14: quality-safe Whisper acceleration

| # | Gate | Result |
| --- | --- | --- |
| 14.1 | Benchmark bundles and replay verifier pass their self-tests | ☑ |
| 14.2 | Current child launch removes inherited loader, layer, cache, and device overrides | ☑ |
| 14.3 | Current managed CPU and system Vulkan product paths execute on Linux | ☑ |
| 14.4 | Exact merged Debian and RPM identities pass the VAD-active full corpus | ☐ `INCOMPLETE` |
| 14.5 | Every required product-speech class is represented | ☑ |
| 14.6 | Hardened reset evidence spans two distinct boot IDs | ☑ |
| 14.7 | Production selection remains managed CPU while any gate is incomplete | ☑ |
| 14.8 | Workspace, frontend, package staging, tag workflow, and responsive regressions are green | ☐ final tag pending |

Acceleration is not ready to ship while 14.4 and 14.8 are open. These are post-merge evidence gates, not product bugs.
