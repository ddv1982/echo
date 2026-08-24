# v0.9 minimal settings and recognizable Linux microphones

## Definition of done

Echo v0.9 passes when all of these statements are true.

1. Settings has no horizontal overflow at 760, 761, 800, 920, and 1024 by 600 pixels. Microphone selection, setup progress, errors, and primary actions remain usable at every width.
2. General shows one compact speech readiness card. Installed paths, alternative models and engines, managed-copy actions, Verify, Repair, and Remove start collapsed.
3. The microphone picker shows system default and primary physical or session-server sources first. Friendly Bluetooth names and transport hints appear when PipeWire or PulseAudio exposes them. ALSA plugins and aliases start collapsed under Advanced audio endpoints.
4. Exact endpoint IDs remain authoritative for capture. Environment and config precedence stay intact. A saved endpoint that is unavailable after a host change remains visible and requires explicit reselection.
5. Preview uses a hostile Linux-shaped inventory and the full setup catalog instead of optimistic fixtures.
6. Installer integrity, cancellation, repair, removal, system runtimes, and manually imported models keep working.

## Research result

Echo compiles CPAL without its optional PipeWire or PulseAudio features. Linux therefore uses ALSA. CPAL's ALSA host enumerates PCM hints, including physical devices, `hw` and `plughw` aliases, resamplers, downmixers, `pulse`, `pipewire`, `dsnoop`, and other plugins. Echo currently deduplicates exact IDs only and renders every entry.

The individual Bluetooth source and its friendly name normally exist as PipeWire or PulseAudio objects. PipeWire provides user-facing `device.description`, `node.description`, and `node.nick` fields. WirePlumber's BlueZ monitor creates connected Bluetooth device and node objects. CPAL 0.18.2 prefers PipeWire, then PulseAudio, then ALSA when those features are compiled and available.

The setup problem is different. The secure installer is sound. Its seven-component catalog is exposed directly as the normal Settings experience. The responsive layout then compounds the problem. The native minimum width is 760 pixels, exactly where the sidebar changes mode, while ordinary setting rows do not stack until an unreachable 520 pixels.

Sources:

- [CPAL ALSA, PipeWire, and PulseAudio guidance](https://github.com/rustaudio/cpal/blob/master/README.md#alsa-pipewire-and-pulseaudio)
- [PipeWire object properties](https://docs.pipewire.org/page_man_pipewire-props_7.html)
- [WirePlumber Bluetooth monitoring](https://pipewire.pages.freedesktop.org/wireplumber/daemon/configuration/bluetooth.html)
- [GNOME adaptive design](https://developer.gnome.org/hig/guidelines/adaptive.html)
- [GNOME boxed lists](https://developer.gnome.org/hig/patterns/containers/boxed-lists.html)

## Rigor

High. The change alters Linux capture-host selection, stored device behavior, Settings information architecture, responsive layout, preview fidelity, packaging dependencies, and release artifacts. The real user hardware is unavailable on this macOS host, so deterministic backend fixtures and Linux CI define the automated Bluetooth proof boundary.

## Phased plan

### Phase 1. Verification scaffold

- Capture the v0.8 screenshot failure and hostile fixture expectations.
- Add one focused verifier for audio projection, setup presentation, preview completeness, and stale layout rules.
- Keep the existing full workspace and release gates unchanged.

### Phase 2. Native Linux audio and product projection

- Compile CPAL PipeWire and PulseAudio support while retaining ALSA fallback.
- Keep CPAL handles private in `audio.rs`.
- Add backend-neutral descriptors and a pure classifier in `microphone.rs`.
- Project friendly label, host, transport, concise hint, and primary or advanced tier.
- Preserve exact IDs, duplicate labels, fallback rules, and unavailable saved selections.
- Update Linux build dependencies and package verification.

### Phase 3. Minimal microphone experience

- Render a distinct system-default choice that names its current source.
- Show primary sources as compact radio rows with recognizable Bluetooth, USB, and built-in hints.
- Keep selected unavailable or advanced endpoints visible.
- Put virtual plugins, aliases, raw IDs, and technical metadata under Advanced audio endpoints.
- Keep Refresh and Test beside the selected summary instead of below a long catalog.

### Phase 4. Minimal speech setup

- Derive one pure presentation model from the existing readiness snapshot.
- Show Ready, Needs setup, In progress, Needs repair, or Unsupported in one card.
- Keep Recommended as the primary action and Parakeet as a secondary alternative.
- Move component inventory and maintenance under Installed components.
- Move inactive engines, alternative models, raw paths, and manual details under Advanced speech options.
- Preserve every existing command and installer invariant unless implementation proves a small operation-label field is necessary.

### Phase 5. Responsive and preview proof

- Move the navigation breakpoint away from the native minimum.
- Constrain Settings width, allow flex and grid children to shrink, wrap status and paths, and make selects fit their container.
- Replace the undefined radius token.
- Make preview contain all seven components, all five plans, Bluetooth, duplicate friendly names, sparse metadata, and many ALSA plugins.
- Prove light and dark layouts at 760, 761, 800, 920, and 1024 by 600 pixels with real browser geometry and recordings.

### Phase 6. Ship

- Run deslop and multi-model interrogation.
- Bump to v0.9.0 and write bounded release notes.
- Open and babysit the PR, fix accepted review comments, and require exact-head green checks.
- Merge, verify the exact main merge, create the annotated v0.9.0 tag, and verify the published release assets.

## Throughput checkpoint

- **Blocking first steps.** The product types, hostile fixtures, and focused verifier land before UI work.
- **Independent workstreams.** Research and competing design sketches ran in parallel. Production code has one owner because audio DTOs, preview data, and Settings rendering share a contract.
- **Shared mutable state.** One implementation owner writes the branch. Reviewers remain read-only until the diff is complete.
- **Smallest safe decomposition.** Six ordered units keep each commit testable while avoiding temporary compatibility layers.

## Claim boundary

The release may claim native PipeWire and PulseAudio discovery, friendly metadata projection, ALSA noise disclosure, responsive browser proof, and preserved capture identity. It must not claim that the user's specific Bluetooth earbuds were live-tested unless that happens on Linux hardware before release.
