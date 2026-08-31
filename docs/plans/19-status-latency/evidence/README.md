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
- Source base: `6998ae063b95cef0bb8346ad15d7ab310f057dbb`

## Deterministic empty fixture

| Lane | p50 | p95 |
| --- | ---: | ---: |
| No-op invoke | <1 ms | 1 ms |
| Fixed `AppStatus` payload | <1 ms | 1 ms |
| Current full status | <1 ms | 1 ms |

The cold backend reconstruction took 19.68 ms. The warm Rust full-status total
is 155 us p50 and 177 us p95. Presentation is 148 us p50 and 165 us p95.

Raw samples: [baseline-empty.json](baseline-empty.json).

## Existing user data

| Lane | p50 | p95 |
| --- | ---: | ---: |
| No-op invoke | <1 ms | 1 ms |
| Fixed `AppStatus` payload | <1 ms | 1 ms |
| Current full status | 19 ms | 20 ms |

The cold backend reconstruction took 2.17 seconds. The warm Rust full-status
total is 18.12 ms p50 and 19.02 ms p95.

| Rust stage | p50 | p95 |
| --- | ---: | ---: |
| Status file | 7 us | 12 us |
| Shortcut | 7 us | 8 us |
| History | 120 us | 145 us |
| Presentation | 17.99 ms | 18.89 ms |

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
