# Verification plan

## Focused Rust checks

- PipeWire Bluetooth sources are primary and retain their friendly description.
- PulseAudio sources are primary even when transport metadata is sparse.
- ALSA physical endpoints remain available when no session server exists.
- ALSA plugins, aliases, resamplers, monitors, and `dsnoop` endpoints are advanced.
- Exact duplicate IDs collapse while duplicate friendly labels remain distinct.
- The selected advanced endpoint remains visible exactly once.
- Environment, config, legacy name, missing selection, fallback, and exact test semantics remain unchanged.
- An unavailable `alsa:*` selection under a native host is retained and never auto-remapped.

## Focused frontend checks

- Ready General shows one speech summary and no raw component path before disclosure.
- Recommended setup is the only primary install action when setup is needed.
- Installed components and Advanced speech options start closed.
- Progress, cancellation, repair, disk errors, and failures stay visible in the summary.
- System default and primary microphones render before Advanced audio endpoints.
- Bluetooth, USB, built-in, duplicate-label, sparse, missing, and virtual fixtures render truthfully.
- Preview includes seven components and five plans.

## Real layout proof

For light and dark themes, exercise 760, 761, 800, 920, and 1024 by 600 pixels. Also exercise one pixel below, at, and above the chosen navigation breakpoint.

At each size:

- `document.documentElement.scrollWidth <= document.documentElement.clientWidth`.
- Every marked Settings surface has `scrollWidth <= clientWidth`.
- The selected microphone, Test action, setup state, primary action or ready state, and error or progress remain visible.
- Installed components and Advanced audio endpoints start closed and can be opened with keyboard and pointer input.

Screenshots and a browser recording are review evidence. Geometry assertions are the pass or fail gate.

## Full gates

- Frontend typecheck, lint, all tests, and production build.
- Rust format, strict workspace clippy, all workspace tests, and release build.
- Existing recording-limit, fixed-toggle, first-run, and transcription verifiers.
- Linux CI package metadata, AppImage smoke test, staged asset validation, exact main checks, tag checks, and published asset digests.

## Hardware boundary

Deterministic descriptors prove projection and disclosure. Linux CI proves native hosts compile and packages contain the required runtime dependencies. Only a real Linux hardware session can prove the user's exact Bluetooth device appears and records, so release notes must keep that distinction explicit.
