# First-run readiness and microphone identity

## Product result

Echo v0.7.0 turns setup into two visible jobs. A user chooses and tests the microphone that Echo will use, then installs or selects a complete local speech setup. Home shows the focused first-run flow until a microphone can open, a speech engine can run, and history contains one successful non-empty dictation.

Settings shows microphones as full-width selectable rows. Each row uses CPAL's stable ID for identity and a readable label plus manufacturer, device type, connection, and default badges when the host supplies them. `Follow system default` remains distinct from pinning the device that happens to be default today. Duplicate labels stay separate. A disconnected saved device stays visible with the actual fallback named. Refresh runs on Settings focus and through an explicit button.

Testing a selected connected device is exact and never falls back. The result names the device and reports input heard, silence, permission, busy, or disconnected state through an `aria-live` region. When a saved device is missing, the UI separately offers to test the effective system fallback.

Speech setup has one Recommended action and one Parakeet action. Recommended installs or reuses a Whisper runtime, a multilingual model, and Silero VAD. Machines with at least 4 GiB RAM get Small. Lower-memory or unknown machines get multilingual Base Q5_1. Large v3 Turbo stays an advanced choice until Echo has measured a reliable memory and latency threshold. Parakeet installs sherpa-onnx 1.13.6 and Parakeet TDT 0.6b v3 INT8.

External components can satisfy a plan. System runtimes and files in the existing model cache are labelled System or External and remain read-only. Users can explicitly install a managed copy. A successful Recommended setup selects Whisper and its recommended model. A successful Parakeet setup selects Parakeet and states that Auto would otherwise prefer an available Parakeet. Environment overrides remain authoritative and the UI names them.

## Microphone domain

Upgrade CPAL to 0.18.2. Core config stores a tagged microphone selection with stable ID and last-seen label. The deserializer still accepts the v0.6 string name. `ECHO_MICROPHONE` accepts an ID first, then a unique exact legacy name.

```rust
enum MicrophoneSelection {
    Device { id: String, last_seen_label: String },
    LegacyName { name: String },
}

struct InputDeviceInfo {
    id: String,
    label: String,
    is_default: bool,
    manufacturer: Option<String>,
    device_type: Option<String>,
    interface_type: Option<String>,
    address: Option<String>,
    driver: Option<String>,
    extended: Vec<String>,
}

enum InputSelectionStatus {
    SystemDefault { active: Option<InputDeviceInfo> },
    Selected { device: InputDeviceInfo },
    MissingWithFallback { requested_id: String, requested_label: String, fallback: InputDeviceInfo },
    MissingWithoutFallback { requested_id: String, requested_label: String },
    AmbiguousLegacyName { name: String, matches: Vec<InputDeviceInfo>, fallback: Option<InputDeviceInfo> },
}
```

Enumeration compares default IDs, deduplicates by ID, merges a separately obtained default handle, and sorts default first, then label, then ID. Device handles stay private to the audio module. Recording re-resolves the stable ID for each session and retains that handle through capture. Missing saved selections preserve current nonfatal fallback behavior, but the backend projects the requested and actual device so the UI never reproduces selection rules.

A dedicated `set_microphone` command changes only this preference, clears legacy data, reloads config, invalidates health, and returns a fresh microphone snapshot. It does not round-trip the entire Settings object.

The CPAL upgrade updates unified error mapping, by-value stream configuration, explicit play, and sample-format coverage. Readiness validates the resolved device's default input configuration. The one-second test remains the stronger audible-input check.

## Managed component domain

The catalog is compiled code. React can submit component or plan IDs, never URLs, archive members, commands, or paths.

```rust
enum ComponentId {
    WhisperRuntime,
    WhisperBaseQ5_1,
    WhisperSmall,
    WhisperLargeV3TurboQ5_0,
    SileroVad,
    SherpaRuntime,
    ParakeetTdt06bV3Int8,
}

enum SetupPlanId {
    Recommended,
    Parakeet,
    WhisperBase,
    WhisperSmall,
    WhisperLargeV3Turbo,
}

enum ManagedComponentState {
    Absent,
    Partial { received: u64, total: u64 },
    Ready { version: String, bytes: u64 },
    NeedsRepair { reason: RepairReason },
    Unsupported { reason: String },
}

struct ComponentStatus {
    id: ComponentId,
    managed: ManagedComponentState,
    external: Vec<ExternalComponent>,
    active: Option<ActiveSource>,
}
```

Managed and external state are orthogonal because both can exist at once. Runtime and model resolution prefer a healthy managed component, then retain the current PATH and manual-cache fallbacks. Legacy bare model names resolve external first for compatibility. New model selections use origin-qualified keys.

Managed installation supports Linux x86_64. Other platforms keep external setup and receive a clear unsupported status.

## Filesystem layout and activation

Managed files stay separate from the existing flat model cache:

```text
$ECHO_MODEL_DIR/
  ggml-small.bin                         external, untouched
  parakeet-tdt-0.6b-v3/                 external, untouched
  managed/
    downloads/<component>-<digest>.part
    downloads/<component>-<digest>.json
    staging/<operation>/<component>/
    components/<component>/releases/<digest>/
      payload/...
      receipt.json
    active/<component>.json
    locks/<component>.lock
```

Every activated revision is immutable. Installation renames a verified staging tree into a digest-named release, then atomically replaces one small activation record. Readers see the old complete revision or the new complete revision. They never scan downloads, staging, inactive releases, receipts, or invalid activation targets.

Receipts list each installed regular file, size, SHA-256, and executable mode. The compiled catalog pins the outer artifact digest, archive root, extraction limits, required members, and payload rules. A generated inventory records exact inner payload hashes for the three production archives. Quick status checks validate activation, receipt identity, required paths, sizes, modes, and unchanged file metadata. Verify and Repair hash every managed payload byte. Invalid receipts trigger repair rather than becoming an integrity authority.

Prepared transcription holds shared component leases for every managed runtime, model, and VAD it uses. Repair and removal take exclusive per-component leases in enum order. This closes the race between the desktop installer, `rec --toggle`, and file transcription.

Removal accepts a `ComponentId`, never a caller path. It deactivates the component atomically, then deletes only validated managed releases, receipts, staging, and partials for that ID. Repeated removal succeeds. System paths, manual files, config, history, and dictionary are never candidates.

## Resumable download and extraction

One operation installs plan components sequentially. A second compatible request returns the active operation. A conflicting request returns Busy without replacing the first cancellation token.

Each stable partial has atomic metadata with component, URL, expected bytes, expected SHA-256, ETag, and Last-Modified. Resume sends `Accept-Encoding: identity`, `Range`, and `If-Range`. Echo appends only to a `206` with the exact start and total. A `200` restarts from zero. A matching complete `416` proceeds to verification. Oversized, short, or contradictory responses fail without activation.

Cancellation, process exit, and network interruption preserve a valid partial. A complete length or SHA-256 failure deletes or quarantines it so Retry starts clean. Downloaded archives remain outside active paths.

Disk admission checks the managed filesystem before network access and before each component. It accounts for remaining download bytes, exact unpacked staging bytes, an existing active revision during repair, and a safety margin equal to the larger of 256 MiB or ten percent. Errors name required and available space.

Extraction runs in Rust through `tar`, `flate2`, and `bzip2`. It enforces the catalogued archive root, members or whole-root rule, entry count, expanded byte limit, and destination containment. It rejects absolute paths, `..`, hard links, devices, FIFOs, sockets, duplicates, and escaping symlinks. Catalogued relative symlinks are created only after regular files. Echo discards archive ownership and special bits, sets known modes, validates required files, and probes runtime entry points before activation.

## Tauri and frontend boundaries

`src-tauri/src/setup.rs` owns one installer service, background worker, cancellation, progress events, config activation, and health invalidation. Installer policy remains in `crates/echo/src/install`. Events are wake-up hints. `get_readiness` is authoritative after restart or missed events.

Replace the model-offer downloader and global download map after the new direct-file path covers every existing offer. Keep `list_models` as the model and language picker input.

The frontend adds focused `MicrophoneChooser`, `FirstRunSetup`, `SpeechSetupCard`, and `ComponentRow` components. Home and Settings consume the same readiness snapshot. Progress is per component. Repair and Remove appear only for managed components. Remove requires a confirmation that names the managed component and reclaimable bytes. Install, Resume, Verify, and Repair do not ask for confirmation.

## Scope limits

v0.7.0 does not add installers for other platforms or architectures, arbitrary artifacts, background updates, parallel downloads, package-manager integration, privilege escalation, a database, persistent microphone test history, or a device notification daemon. It does not adopt, move, hash for ownership, repair, or delete external components.

## Implementation sequence

1. Upgrade CPAL and preserve current behavior. Add stable IDs, metadata, pure resolution, and exact test results.
2. Replace the microphone UI and prove duplicate labels, fallback, refresh, and accessibility.
3. Generate and review the production artifact inventory. Add the closed catalog, hardware recommendation, disk plan, managed paths, activation records, and quick scanner without network mutation.
4. Add injected transport and disk probes. Replace direct-file SHA-1 downloads with resumable SHA-256 downloads and atomic activation.
5. Add bounded archive extraction and the managed Whisper runtime. Complete Recommended setup.
6. Add sherpa runtime, Parakeet model, and Parakeet setup.
7. Add full Verify, Repair, managed-only Remove, component leases, first-run Home flow, offline verifier, documentation, and release notes.

No UI button ships before its cancellation, recovery, integrity, and deletion semantics have fixture tests.

## Synthesis decision

Candidate C supplies the user experience and complete safety contract. Candidate D supplies the smaller installer module shape, injected transport and disk probes, quick snapshots, and component leases. Candidate B supplies explicit microphone resolution variants and external components satisfying plans. Candidate A supplies the dedicated microphone mutation, immutable generations, and receipt-owned removal.

The synthesis rejects candidate A's automatic Turbo tier, candidate C's active symlinks and full hash on every Settings visit, any framework broader than the fixed v0.7 catalog, and any ownership rule inferred from filenames or old cache locations.
