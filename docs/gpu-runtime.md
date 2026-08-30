# Whisper GPU runtime

Echo defaults to CPU transcription. GPU is an explicit Whisper setting, not an
automatic mode and not a property of the application package.

## What happens when GPU is selected

1. Echo installs the managed Whisper CPU runtime if it is missing.
2. Echo downloads the separately published Whisper GPU runtime on demand.
3. Settings lists the Vulkan devices reported by that runtime.
4. Echo stores the selected device and driver UUID pair, so device reordering
   does not silently select different hardware.
5. A failed GPU run is quarantined and retried once on CPU. Settings reports why
   the requested GPU path did not run.

The AppImage, Debian package, RPM, and raw binary use the same managed runtime.
Users who remain on CPU do not download the GPU component.

## Integrity boundary

The component catalog pins the runtime archive URL, byte size, and SHA-256
digest. Installation verifies every catalogued file and symlink before
activating an immutable generation. Repair replaces a missing or corrupt
generation. Remove deletes only inventory-owned files under Echo's managed
cache.

The GPU archive is not part of the desktop SBOM because it is built and
published separately. Its catalog digest, archive receipt, and per-file
inventory are its verification contract.

## Diagnose a fallback

Open **Settings → Advanced** and inspect the last-run acceleration result. Echo
distinguishes a missing runtime, no Vulkan devices, an absent pinned device, a
quarantined device, a missing CPU fallback, and a GPU run that failed before a
CPU retry.

If the runtime is missing or corrupt, use the repair action in Settings. If the
device disappeared after a driver or hardware change, choose a currently listed
device again.

Maintainers build and publish the archive independently of application tags.
See [RELEASING.md](RELEASING.md#publish-the-whisper-gpu-runtime-archive) for the
reproducible packaging and catalog update procedure.
