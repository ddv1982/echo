# First-run readiness verification

## Baseline

- The v0.6 Settings microphone control is one narrow name-based select. The browser baseline is `/Users/vriesd/.t3/userdata/browser-artifacts/browser-recording-mt5eqsr1.mp4`.
- `cargo test -p echo audio::tests` has eight passing hardware-free tests.
- The v0.6 downloader verifies four direct files with SHA-1, deletes cancelled partials, and cannot resume, repair, remove, or install runtimes.

## Microphone gates

- CPAL stable IDs round-trip through config and select duplicate labels independently.
- System default, selected, missing with fallback, missing without fallback, unique legacy name, ambiguous legacy name, no devices, and a default omitted from enumeration have table tests.
- Config reads v0.6 strings and writes the tagged ID plus last-seen label only after explicit selection.
- Environment selection accepts ID or unique legacy name, remains locked, and reports fallback truthfully.
- Device metadata maps non-exhaustive CPAL values without hiding a device when optional metadata fails.
- Recording keeps missing-device fallback. Exact Test refuses fallback. Test fallback is a separate explicit action.
- CPAL 0.18 error kinds name permission, busy, disconnected, unsupported, and host failure.
- Stream tests cover every supported sample format after the 0.18 default-format change.
- React renders device rows with unique ID keys, readable metadata, default badges, disconnected state, focus/manual refresh, keyboard control, and `aria-live` heard/silent/error results.
- The ignored live test records from a stable ID and reports the same effective ID.

## Catalog and hardware gates

- Every component has a unique typed ID, HTTPS URL, version, exact bytes, lowercase 64-character SHA-256, format, archive rule, required files, installed bytes, and managed destinations.
- A generator inventories the three pinned archives and records members, expanded sizes, modes, symlinks, and payload SHA-256 values.
- The checked-in catalog matches the grounding hashes and generated inventory.
- Recommendation tests cover unsupported platform, unknown memory, below 4 GiB, exactly 4 GiB, and larger machines. Turbo is never automatic.
- External ready components satisfy plans unless the user explicitly requests a managed copy.
- Disk calculations cover fresh install, resumed partial, archive expansion, repair overlap, exact threshold, unknown free space, and a safety margin.
- Insufficient space causes zero HTTP requests.

## Transfer gates

- Local HTTP fixtures cover fresh `200`, valid `206`, ETag and Last-Modified `If-Range`, server-ignored Range restart, exact complete `416`, mismatched Content-Range, oversized partial, oversized response, short response, disconnect and resume, cancellation and resume, and duplicate-operation rejection.
- Cancellation and interruption preserve a stable partial and valid atomic metadata.
- Exact size and SHA-256 pass before extraction. Any mismatch leaves no active component and no endlessly reusable corrupt partial.
- Progress includes operation, component, phase, bytes, total, and resumed offset. Phase changes are never throttled away.

## Extraction and activation gates

- Tiny direct-file, tar.gz, and tar.bz2 fixtures install through the production path.
- Extraction rejects absolute paths, parent traversal, duplicate normalized paths, hard links, devices, FIFOs, sockets, unexpected members, escaping symlinks, symlink-parent traversal, entry overflow, expanded-byte overflow, wrong payload hash, wrong ELF architecture, and missing required files.
- The runtime runner and libraries receive catalogued modes. Runtime probing happens only after all validation passes.
- Failure injection after partial, artifact verification, extraction, release rename, activation temp write, activation replacement, and old-release cleanup always leaves the old valid revision or no active revision.
- Restart discards staging, keeps valid partials, ignores inactive releases, and never promotes by assumption.
- Quick status finds invalid records, missing files, wrong sizes, and wrong modes. Verify and Repair detect same-size content corruption.

## Repair, removal, and coexistence gates

- Repair skips a healthy component, replaces each corrupt or missing payload in turn, and leaves the previous good activation until replacement succeeds.
- Managed runtime/model/VAD resolution wins when healthy. Corrupt managed state falls back to the exact previous system or manual candidate and remains visible as Needs Repair.
- Prepared transcription holds sorted shared component leases. Repair and removal cannot race an active inference.
- Removal accepts component IDs only, is idempotent, validates containment and receipt identity, and deletes only managed activation, releases, staging, and partials.
- Manual cache sentinels, system runtime sentinels, config, history, status, and dictionary byte-compare unchanged after repair and removal.
- Successful Recommended setup selects Whisper and its model. Successful Parakeet setup selects Parakeet. Environment overrides remain effective and visible.

## UI and first-run gates

- Empty Linux x64 state shows microphone and Recommended speech setup together.
- Component rows independently show Missing, Partial, Downloading, Verifying, Extracting, Ready, Needs Repair, External, System, Failed, Cancelled, and Unsupported states.
- Cancel becomes Resume with retained bytes. Retry after checksum failure starts clean.
- Repair and Remove appear only for managed components. Remove confirmation names the target and reclaimed bytes.
- Unsupported platforms show external/manual guidance and no managed mutation buttons.
- First-run state derives from microphone resolution, usable speech inventory, and an existing successful non-empty history row.
- The view works at desktop and narrow widths, light and dark themes, reduced motion, keyboard navigation, and screen-reader announcements.

## Rerunnable proof

`scripts/verify-first-run-readiness.sh` must use isolated config/model/data/PATH roots, a local Range-aware fixture server, tiny artifacts, and fake runtime executables. It performs an empty-cache Recommended install, cancellation, process restart, resume, SHA-256 verification, activation, managed engine resolution, corruption, Repair, removal, and external-file survival. A fake device adapter proves duplicate-label stable-ID selection and actual fallback projection. It never opens host audio or reaches the public internet.

The release gate also runs frontend build, lint, Node 22 tests, clippy, workspace tests on Linux, existing fixed-toggle and transcription verifiers, release build, package assembly, and the tagged release workflow.
