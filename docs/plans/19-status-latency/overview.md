# Status latency architecture

## Problem

The desktop calls `get_app_status` through Tauri, then waits 400 ms after the
request settles before it polls again. The command currently rebuilds
`AppStatus` from status files, history, configuration, shortcut state, health
probes, and the speech runtime inventory.

The existing 472.55 ms result does not isolate IPC cost. It comes from a debug
unit test that uses Tauri's mock dispatcher. Test builds also reload config from
disk where production uses a cached snapshot. We must measure a release WebView
before setting an optimization target.

## Research constraints

- Tauri commands provide typed request and response IPC. Tauri recommends async
  commands for work that must not block the main thread.
- Tauri events fit small, low-rate state changes. They use JSON at runtime and
  do not provide an ordered high-throughput stream.
- Tauri Channels solve ordered streaming. Echo status is latest state, so a
  Channel adds lifecycle and replay rules without removing file reconciliation.
- React subscriptions must unsubscribe after every effect lifetime, including
  the development-only Strict Mode setup, cleanup, and setup replay.
- `tokio::sync::watch` retains the latest value and fits a materialized status
  snapshot.
- The recorder CLI writes status and history files in another process. Desktop
  events cannot replace reconciliation with these files.

Research sources:

- [Calling Rust from the frontend](https://v2.tauri.app/develop/calling-rust/)
- [Calling the frontend from Rust](https://v2.tauri.app/develop/calling-frontend/)
- [Tauri IPC](https://v2.tauri.app/concept/inter-process-communication/)
- [Tauri state management](https://v2.tauri.app/develop/state-management/)
- [React `useEffect`](https://react.dev/reference/react/useEffect)
- [`tokio::sync::watch`](https://docs.rs/tokio/latest/tokio/sync/watch/)
- [`tracing`](https://docs.rs/tracing/latest/tracing/)

## Caller view

The frontend learns one subscription operation. The adapter owns event ordering,
the initial command read, recovery polling, and cleanup.

```ts
export interface DesktopApi {
  watchAppStatus(handler: (status: AppStatus) => void): Promise<() => void>
}
```

The backend exposes a prepared snapshot and semantic invalidation. Callers do
not select cache fields.

```rust
pub struct AppStatusUpdate {
    pub sequence: u64,
    pub status: AppStatus,
}

pub struct StatusService;

impl StatusService {
    pub fn current(&self) -> Arc<AppStatusUpdate>;
    pub fn changed(&self, change: StatusChange) -> ChangeReceipt;
}
```

`ChangeReceipt` lets a completed transaction wait until its effect appears in a
published snapshot. Recording commands that start asynchronous work do not
promise immediate reflection. The subsequent status update is authoritative.

## Chosen shape

One async coordinator owns the status cache and its sequence. It publishes the
latest `Arc<AppStatusUpdate>` through `tokio::sync::watch`. The Tauri command
returns a clone. The command performs no file reads, history parsing, hardware
probes, `gsettings` calls, or runtime inventory scans.

The coordinator caches these private facets:

- activity and session state;
- the projected last history row;
- configuration and presentation fields;
- speech readiness and language warning;
- microphone, injection, executable, and install health;
- shortcut state and activation;
- static policy, version, and path fields.

Local mutations send `StatusChange` values. A 250 ms metadata loop reconciles
the status, history, config, and shortcut-activation files. A ten-second deadline
refreshes microphone, PATH, runtime, and install health.

File stamps contain the device, inode, length, nanosecond mtime, and ctime when
available. A loader reads a stamp before and after parsing. If the stamp changes,
the loader retries once and leaves the facet dirty when it cannot get a stable
read.

The actor initially serializes refreshes. If measurement shows that history,
speech, or environment probes delay session convergence, only those collectors
move to generation-checked `spawn_blocking` tasks. Filesystem watching is not in
the initial design. It may become an optional wake source if the fixed metadata
loop fails a measured latency or idle-cost gate.

## Delivery and ordering

The actor emits the full generated `AppStatusUpdate` after a semantic change.
The event rate is low and the payload is modest. `get_app_status` remains the
typed bootstrap, manual refresh, recovery, and benchmark command.

The frontend adapter:

1. registers the event listener;
2. reads the initial snapshot;
3. rejects a sequence that is not newer;
4. keeps one in-flight and one queued recovery read;
5. reads immediately when the document becomes visible;
6. keeps a five-second visible fallback;
7. disposes a late listener registration after unmount.

`LastRun.id` is added to the generated status payload. The application refreshes
History and Dictionary when this durable ID changes. It no longer depends on
observing every intermediate recording phase.

## Invalidation matrix

| Cause | Facets | Source of truth | Target |
| --- | --- | --- | --- |
| Status file replacement or recorder death | Activity | Status stamp and live PID | 500 ms |
| History replacement | Last run | History stamp | 500 ms |
| Settings save or external config edit | Presentation, speech, readiness | Semantic hint and config stamp | Immediate or 500 ms |
| Setup, repair, remove, or verify | Speech, readiness | Semantic hint and health deadline | Immediate or 10 s |
| Shortcut state or activation | Shortcut | Semantic hint, native revision, activation stamp | 500 ms |
| Microphone or PATH drift | Readiness | Health deadline | 10 s |
| No input change | None | Metadata only | No publication |

## Module map

- `crates/echo-ipc/src/lib.rs`: `AppStatusUpdate` and `LastRun.id`.
- `crates/echo/src/stt/status.rs`: one runtime inventory produces speech status.
- `src-tauri/src/status/mod.rs`: `StatusService`, actor, cache, projection, and
  publication.
- `src-tauri/src/status/sources.rs`: file stamps and external source loaders.
- `src-tauri/src/status/tests.rs`: invalidation, race, and convergence tests.
- `src-tauri/src/commands/status.rs`: async cached snapshot command.
- `frontend/src/api/DesktopApi.ts`: `watchAppStatus`.
- `frontend/src/api/tauriDesktopApi.ts`: listener-first event and command adapter.
- `frontend/src/api/previewDesktopApi.ts`: in-memory status feed.
- `frontend/src/app/useAppController.ts`: applies status and reacts to
  `LastRun.id` changes.

## Performance contract

Phase 19.1 records release-profile raw samples from the real WebView:

- no-op command transport;
- fixed `AppStatusUpdate` serialization;
- current complete status call;
- warm and cold backend stage spans;
- external writer convergence;
- idle CPU and wakeups.

The initial treatment gates are:

- cached p95 is at most `max(no-op p95 + 5 ms, 15 ms)`;
- cached p50 is at most `max(no-op p50 + 2 ms, 8 ms)`;
- 500 cached reads perform no history load or runtime inventory scan;
- local invalidation reaches the WebView within 100 ms p95;
- external status and history changes converge within 500 ms p95;
- idle CPU grows by no more than 0.2 percentage points or 5 percent;
- no-op transport p95 does not regress by more than 10 percent.

Phase 19.1 may tighten these limits from stable baseline evidence. It may not
loosen them to hide a treatment regression.

## Synthesis decision

Three independent designs converged on a single-owner materialized snapshot. An
independent judge selected the fixed-reconciliation design as the base, scoring
it 29 of 30.

The synthesis keeps:

- stable before/read/after file stamps and frontend lifecycle rules from the
  second design;
- `LastRun.id` from the third design;
- generation-checked collectors only when measurement shows blocking;
- full generated status events rather than revision-only events;
- a fixed metadata loop rather than watcher-first reconciliation.

Rejected designs include high-frequency polling with local memoization, events
without file reconciliation, a Tauri Channel, revision-only events that require
another invoke, watcher-first reconciliation, and multiple writable caches.

## Risks

- The reference runner must pin WebKitGTK, Tauri, the CPU governor, and the
  desktop session before an absolute latency gate is trustworthy.
- Filesystems without useful inode and nanosecond timestamp data may need a
  digest fallback for the small status, config, and activation files.
- A corrupt history refresh must preserve today's set-aside behavior.
- Consumers that need every transient phase require a separate domain event.
  `AppStatus` remains latest state, not an event log.
