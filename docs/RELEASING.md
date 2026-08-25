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

### Stage a qualified Whisper acceleration release

Use this path when the release contains an admitted Whisper accelerator. Do not let the tag workflow rebuild the qualified executable.

1. Build `target/release/echo-desktop` from clean `origin/main`. Set `ECHO_BUILD_COMMIT` to the full main commit.
2. Run `patch-tauri-bundle-type.py` to create the Debian and RPM ELF variants.
3. Run the full Whisper qualification against each variant.
4. Run `promote-whisper-admission.py` for each passing sweep.
5. Run `stage-qualified-whisper-release.py` to bundle and verify the exact variants.
6. Create a draft GitHub Release named `qualification-$commit`. Upload every staged file to that draft.

The staging command has this shape:

```sh
python3 scripts/stage-qualified-whisper-release.py \
  --canonical-binary target/release/echo-desktop \
  --deb-promotion target/whisper-release/deb-promotion \
  --rpm-promotion target/whisper-release/rpm-promotion \
  --output target/whisper-release/assets \
  --version X.Y.Z \
  --commit "$commit"

gh release create "qualification-$commit" \
  --draft \
  --target "$commit" \
  --title "Qualified release candidate $commit" \
  target/whisper-release/assets/*
```

The tag workflow downloads that draft. It extracts both packages and checks the executable, admission, runtime, and cache identities before it publishes the final release.

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
versions, and uploads those artifacts plus `echo-desktop`. A release-candidate
job downloads the artifacts and verifies the exact directory layout consumed
by the publisher. A separate job with release-write permission creates the
GitHub Release only after all required artifacts are available.

## Verify

Confirm that the workflow is green and the Release has all three required
assets plus `qualified-release.json` for an acceleration release:

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
