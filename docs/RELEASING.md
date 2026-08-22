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
assets:

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
