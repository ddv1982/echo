# Phase 12: Make releases assert what they ship

[Back to overview](overview.md)

## Goal

Stop a release from silently shipping less than the changelog claims, which is how 0.12.6 published with no acceleration payload and no check objected.

## Changes

**`.github/workflows/release.yml`.** Remove the qualification-draft substitution path entirely: the `Detect qualification draft` probe, the `qualified-release-assets` job, and the branch that downloads a hand-built draft and discards the CI build. Add an assertion that the published deb, rpm, and AppImage each contain the binary and the desktop entry, and that the build is the one CI produced.

**`docs/RELEASING.md`.** Delete the staged-qualification procedure. Replace it with the Vulkan runtime archive's own lifecycle: built by an operator on a Vulkan host with `scripts/build-whisper-vulkan-receipt.sh`, published once per runtime version, and referenced by digest from the component catalog.

**`CHANGELOG.md`.** Record the user-visible change: acceleration is a CPU or GPU choice with a device picker, defaults to CPU, and downloads its runtime on demand.

**`docs/plans/16-portable-whisper-acceleration/overview.md`.** Add a continuation line pointing at this plan, matching how plan 12 points at plan 13. Plan 16 has 165 unchecked boxes and its release-simplification half landed ahead of the feature it depended on, which is the direct cause of the 0.12.6 regression.

## Data structures

None.

## Verification

Static:

- `python3 scripts/changelog-notes.py --self-test` passes.
- A workflow dry run proves a build missing its binary fails the release rather than publishing.
- `grep -r qualification- .github/workflows/` returns nothing.

Runtime: cut a prerelease tag, download all three published artifacts, and confirm each installs and dictates on CPU with no acceleration component present. Then install the Vulkan component through Settings and confirm GPU works from the released build rather than a development build.
