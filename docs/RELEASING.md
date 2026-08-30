# Releasing Echo

The release workflow builds packages for every pull request, every push to
`main`, every `v*` tag, and the nightly schedule. A tag is allowed to publish
only after the required Linux packages have been built and inspected.

## Repository gate

Protect `main` and require these pull-request checks before merging:

- `check / check`
- `release / release-policy`
- `release / linux-packages`
- `release / release-assets`

`release / appimage` remains best effort and must not be required. The tagged
release does not attach an AppImage until that job has proved reliable enough
to become a release requirement.

## Prepare the release

1. Bump `workspace.package.version` in `Cargo.toml`.
2. Add a `## vX.Y.Z` or `## vX.Y.Z-alpha.1` section to `CHANGELOG.md`.
3. Open a pull request and wait for both the normal checks and the Linux
   package build to pass.
4. Merge the pull request, then wait for the `main` release workflow run to
   pass. This proves the exact merged commit packages successfully.

## Publish

Whisper acceleration is a two-state choice. Unset means CPU. GPU runs on the
device selected in Settings, pinned by its Vulkan device and driver UUID pair,
and pulls the `Whisper GPU runtime` component on demand. No application release
carries an acceleration payload, so nothing about the GPU path depends on which
tag a user installed. Automatic language and recognition hints run on the same
backend as the rest of the decode.

### Publish the Whisper GPU runtime archive

`echo-whisper-vulkan-runtime.tar.gz` is not built by CI and is not attached to
an application release. An operator builds it once per whisper.cpp revision on a
Vulkan host, publishes it under its own tag, and the component catalog then
references it by digest. Users download it the first time they select GPU.

1. Build the runtime from a clean whisper.cpp checkout at the supported commit:

```sh
scripts/build-whisper-vulkan-receipt.sh /path/to/whisper.cpp target/whisper-vulkan/runtime
```

2. Package it reproducibly. The flags are the point: sorted names, zeroed
   ownership and timestamps, and `gzip -n` are what let a second operator
   rebuild the same tree and get the same digest instead of trusting yours.

```sh
cd target/whisper-vulkan
tar --sort=name --owner=0 --group=0 --numeric-owner --mtime='@0' \
    --exclude=cmake-cache.txt -cf - runtime \
  | gzip -n -9 > echo-whisper-vulkan-runtime.tar.gz
sha256sum echo-whisper-vulkan-runtime.tar.gz
stat -c %s echo-whisper-vulkan-runtime.tar.gz
```

3. Update the `WhisperVulkanRuntime` entry in
   `crates/echo/src/install/catalog.rs`: `version`, `url`, `artifact_size`, and
   `artifact_sha256`. The installer refuses a download that misses either.

4. If the payload changed, regenerate the per-file inventory. The installer
   verifies every file and symlink in the extracted tree against it:

```sh
python3 scripts/generate-managed-inventory.py <dir-holding-every-managed-archive> \
  > crates/echo/src/install/archive_inventory.json
```

5. Prove the archive installs through the real installer before publishing it:

```sh
ECHO_PINNED_VULKAN_ARCHIVE=$PWD/echo-whisper-vulkan-runtime.tar.gz \
  cargo test -p echo --lib install::tests::pinned_vulkan_runtime_archive_installs \
  -- --ignored --exact
```

6. Publish under a runtime tag, not an application tag, at the URL the catalog
   now names:

```sh
gh release create whisper-vulkan-runtime-1.9.2 \
  --title "Whisper Vulkan runtime 1.9.2" \
  echo-whisper-vulkan-runtime.tar.gz
```

The archive has to exist at that URL before a build pointing at it reaches
users. Ship the catalog change in an ordinary application release afterwards.

### Push the application tag

Create an annotated tag on the tested `origin/main` commit and push only the
tag. Do not create a GitHub Release or upload assets by hand.

```sh
git fetch origin main
git tag -a vX.Y.Z origin/main -m "Echo vX.Y.Z"
git push origin vX.Y.Z
```

The tag workflow checks that the tag is on `main`, matches the workspace
version, and has release notes. It then builds with the pinned Tauri CLI,
requires exactly one Debian package and one RPM, verifies their embedded
versions, and uploads those artifacts plus `echo-desktop`. Before publishing it
opens every artefact and checks that the deb and the rpm each contain the
launcher, the desktop entry, and the exact binary that run built, so a package
missing its payload fails the release instead of shipping.

## Verify

Confirm that the workflow is green and the Release has a Debian package, an
RPM, and `echo-desktop`. Include the AppImage when that job succeeds.

```sh
gh run list --workflow release.yml --limit 5
gh release view vX.Y.Z
```

Download the assets and confirm the visible and package versions:

```sh
release_dir=$(mktemp -d)
gh release download vX.Y.Z --dir "$release_dir"
dpkg-deb -f "$release_dir"/*.deb Version
chmod +x "$release_dir/echo-desktop"
"$release_dir/echo-desktop" --version
```

## If a tag run fails

Do not move or reuse a published tag, and do not upload artifacts from a dirty
working tree. Fix the issue on `main`, repeat the package gate, bump to the next
patch version, and create a new tag. This keeps every public tag tied to one
reviewed commit and one reproducible workflow run.
