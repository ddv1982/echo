# Phase 19.1 baseline

The benchmark uses a release build and the real Tauri WebView invoke path under
Xvfb. The browser records transport durations with `performance.now()`. Rust
records status stages in microseconds.
The report keeps the first uncached backend reconstruction separately as
`coldStatusStage`; the 40 entries in `statusStages` remain warm samples.

Reference environment:

- CPU: 12th Gen Intel Core i7-12700H
- WebKitGTK: 2.52.3
- WebView user agent: AppleWebKit 605.1.15, Version 60.5
- Platform: Linux x86_64, X11
- Source base: `5c82378523aa138216b84366ff000594c8f083ae`

## Deterministic empty fixture

| Lane | p50 | p95 |
| --- | ---: | ---: |
| No-op invoke | <1 ms | 1 ms |
| Fixed `AppStatus` payload | <1 ms | 1 ms |
| Current full status | <1 ms | 1 ms |

Rust full-status total: 146 us p50 and 170 us p95. Presentation is 138 us
p50 and 162 us p95.

Raw samples: [baseline-empty.json](baseline-empty.json).

## Existing user data

| Lane | p50 | p95 |
| --- | ---: | ---: |
| No-op invoke | <1 ms | 1 ms |
| Fixed `AppStatus` payload | <1 ms | 1 ms |
| Current full status | 21 ms | 21 ms |

Rust full-status total: 19.66 ms p50 and 20.44 ms p95.

| Rust stage | p50 | p95 |
| --- | ---: | ---: |
| Status file | 8 us | 15 us |
| Shortcut | 9 us | 12 us |
| History | 135 us | 162 us |
| Presentation | 19.51 ms | 20.28 ms |

The presentation stage includes cleanup, HUD, and `language_warning`. The
language warning rebuilds the speech runtime inventory. It accounts for almost
all warm backend latency on this machine. History parsing is not a current
latency problem, but its cost still grows with the capped history file.

Raw samples: [baseline-existing.json](baseline-existing.json).

## Decision

The old 472.55 ms debug/mock result overstates this release path by more than a
factor of 20. Tauri transport and `AppStatus` serialization are below the
browser timer's 1 ms resolution. Phase 19.2 still materializes a cached snapshot
because a synchronous 20 ms runtime scan runs on the command path at every poll.
Phase 19.3 targets the presentation and speech facets first.
