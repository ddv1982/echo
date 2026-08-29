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

Unset Whisper acceleration is Auto. Auto uses a receipt-verified Vulkan
device when one enumerates, otherwise managed CPU. CPU in Settings remains
an explicit opt-out and always passes `--no-gpu`. Automatic language and
recognition hints run on the same backend as the rest of the decode.

### Stage a qualified Whisper acceleration release

Use this path when the release contains an admitted Whisper accelerator. Do not let the tag workflow rebuild the qualified executable.

1. Build `target/release/echo-desktop` from clean `origin/main`. Set `ECHO_BUILD_COMMIT` to the full main commit.
2. Run `patch-tauri-bundle-type.py` to create the Debian and RPM ELF variants.
3. Run the full Whisper qualification against each variant.
4. Run `promote-whisper-admission.py --package-type deb` or `--package-type rpm` for each passing sweep.
5. Compose the Small and Large promotions for each package type with `compose-whisper-admission-set.py`. The composer requires matching binaries, runtimes, probes, VAD contracts, and package types.
6. Run `stage-qualified-whisper-release.py` with the two composed promotions. Staging verifies every record, cache seed, runtime file, and inventory entry in the source and extracted packages.
7. Create a draft GitHub Release named `qualification-$commit`. Upload every staged file to that draft.

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

Compose each package type before staging:

```sh
python3 scripts/compose-whisper-admission-set.py \
  --promotion target/whisper-release/deb-small \
  --promotion target/whisper-release/deb-large \
  --output target/whisper-release/deb-promotion
```

The tag workflow downloads that draft. It extracts both packages and checks the executable, admission set, shared runtime, full inventory, and every cache identity before it publishes the final release.

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
versions, and uploads those artifacts plus `echo-desktop`. If a
`qualification-$commit` draft exists, the publisher attaches those qualified
packages instead. If it does not, the publisher attaches the Linux packages
from that tag run.

## Verify

Confirm that the workflow is green and the Release has a Debian package, an
RPM, and `echo-desktop`. Include the AppImage when that job succeeds. A
qualified acceleration release also includes `qualified-release.json`:

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
