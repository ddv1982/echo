# QA gates

**Last QA merge:** [2026-08-25 Phase 14 follow-up](runs/COORDINATOR-MERGE-2026-08-25-2.md)

## Gate 14: quality-safe Whisper acceleration (closed, superseded)

Closed unmet. Gates 14.4 and 14.8 were never satisfied, and
[Plan 17](../plans/17-selectable-gpu-acceleration/overview.md) removed what they
gated: acceleration ships in v0.13.0 as a CPU or GPU choice the user makes, with
a device picker and a runtime downloaded on demand, so there is no admission to
qualify per host. The table below is the state it was frozen in.

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

At the time of freezing: acceleration was not ready to ship while 14.4 and 14.8
were open. These were post-merge evidence gates, not product bugs.
