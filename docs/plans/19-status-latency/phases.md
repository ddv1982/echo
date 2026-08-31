# Status latency implementation phases

## 19.0 Lock the architecture

- Commit the caller API, cache ownership, invalidation matrix, module map, and
  performance contract.
- Keep the decision trail with the plan.
- Do not change runtime behavior.

Exit: documentation links and plan checks pass.

## 19.1 Measure the release WebView

- Add a non-shipping `status-perf-probe` feature.
- Register no-op and fixed-payload benchmark commands only under that feature.
- Drive the real release WebView through `@tauri-apps/api`.
- Add structured stage timing to the current status path.
- Record raw samples, p50, p95, cold startup, convergence, CPU, and wakeups.
- Keep the existing mock probe, but label it debug-only.

Exit: transport, serialization, and backend costs are independently measured.

## 19.2 Materialize a cached snapshot

- Add generated `AppStatusUpdate` and `LastRun.id` types.
- Add Tauri-managed `StatusService` and one async coordinator.
- Publish the latest snapshot through `tokio::sync::watch`.
- Start one 250 ms coordinator tick that rebuilds the complete existing status
  projection on `spawn_blocking`. Keep at most one rebuild in flight.
- Retain the last good snapshot when a rebuild fails.
- Make `get_app_status` an async cached clone only after the initial snapshot
  and the repeating rebuild path are active.
- Keep the existing 400 ms frontend poll during this phase.

Exit: the cached command passes the transport-relative gate and does no backend
I/O. Recording, settings, health, and history continue to update while the
frontend polls the cache.

## 19.3 Add facets and reconciliation

- Split the cache into activity, last-run, presentation, speech, readiness,
  shortcut, and static facets.
- Add stable file stamps and the 250 ms metadata loop.
- Refresh health on its ten-second deadline.
- Build speech readiness and language warning from one config and one runtime
  inventory.
- Parse history only after its stamp changes.
- Add semantic `StatusChange` hints and causal `ChangeReceipt` acknowledgements.
- Replace the transitional full-snapshot rebuild tick after every facet has a
  reconciliation or invalidation path.
- Refresh History and Dictionary from `LastRun.id`.
- Test external recorder writes, writer death, atomic replacements, races, and
  coalesced phases.

Exit: every invalidation row has a deterministic test and external changes
converge within 500 ms p95.

## 19.4 Switch to event-first delivery

- Generate the `app-status` event contract.
- Emit a full `AppStatusUpdate` after semantic changes.
- Add `watchAppStatus` to the desktop API and preview adapter.
- Subscribe before the initial command read.
- Reject stale sequences and coalesce recovery reads.
- Add a five-second visible fallback and an immediate visibility refresh.
- Remove the 400 ms status poll. Keep recording-level polling separate.
- Prove Strict Mode and late-listener cleanup.

Exit: local changes render within 100 ms p95 and missed events recover without
duplicate listeners or timers.

## 19.5 Tune and remove transitional code

- Repeat the matched release benchmark.
- Add generation-checked collectors only for stages that block reconciliation.
- Add a filesystem watcher only if fixed metadata polling misses a measured
  gate.
- Delete the old `HEALTH` cache, synchronous status assembly, phase-edge history
  refresh, and transitional poll.
- Run the full Rust, frontend, IPC, release, live, and performance gates.

Exit: all performance limits pass at the integrated head and no duplicate status
implementation remains.
