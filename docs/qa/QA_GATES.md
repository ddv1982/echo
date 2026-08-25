# QA gates

**Last QA merge:** [2026-08-25 Phase 14](runs/COORDINATOR-MERGE-2026-08-25.md)

## Gate 14: quality-safe Whisper acceleration

| # | Gate | Result |
| --- | --- | --- |
| 14.1 | Benchmark bundles and replay verifier pass their self-tests | ☑ |
| 14.2 | Current child launch removes inherited loader, layer, cache, and device overrides | ☑ |
| 14.3 | Current managed CPU and system Vulkan product paths execute on Linux | ☑ |
| 14.4 | Current exact identity passes the full licensed corpus | ☐ `INCOMPLETE` |
| 14.5 | Every required product-speech class is represented | ☐ `INCOMPLETE` |
| 14.6 | Reset evidence spans two distinct boot IDs | ☐ `INCOMPLETE` |
| 14.7 | Production selection remains managed CPU while any gate is incomplete | ☑ |
| 14.8 | Workspace, frontend, release, and responsive regressions are green | ☑ |

Acceleration is not ready to ship while 14.4 through 14.6 are open. These are evidence gates, not product bugs.
