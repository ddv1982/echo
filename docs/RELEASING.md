# Releasing Echo

The release workflow runs its policy checks for every pull request. Package
proof runs for pushes to `main` and `v*` tags, the nightly schedule, and manual
workflow dispatches, not for pull requests. GitHub Releases are the verified
downloads. Debian and Ubuntu can also install from a signed APT repository on
GitHub Pages; that channel is tag-only and does not replace SHA256SUMS or
attestations. A Git tag without a corresponding GitHub Release marks source
history only.

## Repository gate

Protect `main` and require these pull-request checks before merging:

- `check / check`
- `release / release-policy`

`check / check` is the aggregate result of the policy, frontend, Rust, and asset
jobs. The AppImage remains a required package gate. On non-PR events,
`release-assets` waits for both package build jobs and verifies the same
eight-file publish directory on `main`, tags, nightlies, and manual dispatches.

Pin each third-party action to a full commit SHA. Dependabot checks action pins
plus Cargo and npm dependencies each week and opens reviewable pull requests.
The workflow pin check fails if a workflow uses a tag, branch, or short SHA.

CI audits the Cargo and npm lockfiles and fails on vulnerability findings;
warning-class RustSec advisories remain visible without being promoted to
failures. `RUSTSEC-2024-0429` currently concerns `glib 0.18.5`, which is
transitive through the current Tauri/GTK graph, and Echo does not use
`VariantStrIter`. Remove or re-evaluate this note when a compatible dependency
graph no longer resolves the affected `glib` version.

Create a repository ruleset for tags that match `v*`. Restrict tag updates and
deletions, then limit bypass access to the release operators. The workflow also
rejects a tag when retained workflow history shows that the same name pointed
at another commit. Workflow history expires, so the check does not replace the
repository ruleset.

## Prepare the release

1. Bump `workspace.package.version` in `Cargo.toml`.
2. Add a `## vX.Y.Z` or `## vX.Y.Z - YYYY-MM-DD` section to `CHANGELOG.md`, and set the matching `<release>` in `packaging/io.github.ddv1982.echo.metainfo.xml`.
3. When changing an entry in `crates/echo/src/install/catalog.rs`, update its
   URL, digest, license, supplier/source provenance, and the corresponding
   attribution in [`THIRD_PARTY.md`](../THIRD_PARTY.md) in the same pull
   request.
4. Open a pull request and wait for the required `check / check` and
   `release / release-policy` contexts to pass.
5. Merge the pull request, then wait for the `main` release workflow run for
   the exact merged `main` SHA to pass every package gate before tagging. This
   proves that exact commit builds and verifies the Linux packages, AppImage,
   staged release assets, and attestations successfully.

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
The archive bytes are separate from application assets. The desktop SBOM does
record the component's catalog URL, digest, license, supplier, and source
attribution. Its receipt, catalog size, catalog SHA-256 digest, and per-file
inventory remain the archive's separate verification contract. Update
[`THIRD_PARTY.md`](../THIRD_PARTY.md) whenever its provenance or attribution
changes.

### Push the application tag

Create an annotated tag on the tested `origin/main` commit and push only the
tag. Do not create a GitHub Release or upload assets by hand.

```sh
git fetch origin main
git tag -a vX.Y.Z origin/main -m "Echo vX.Y.Z"
git push origin vX.Y.Z
```

The tag workflow checks that the tag is on `main`, matches the workspace
version, and has release notes. It builds with the pinned Tauri CLI and requires
exactly one Debian package, one RPM, one AppImage, and one raw binary. The
workflow checks package metadata and contents. It also checks the final
AppImage desktop entry, executable, and reported version.

After the Tauri `.deb` and `.rpm` bundles, a post-bundle rewrite sets the
Debian `Package` and RPM `Name` to `echo` and renames the artifacts. The
desktop file stays `io.github.ddv1982.echo.desktop` and the binary stays
`echo-desktop`. The rewritten control includes `Replaces`, `Conflicts`, and
`Provides` for `io.github.ddv1982.echo`. The first cutover is `v1.0.0`
(major). Do not retag `0.14.24`.

The workflow stages those four application files, the MIT license,
`THIRD_PARTY.md`, and `echo-desktop.cdx.json` in one directory. The CycloneDX
SBOM lists every Cargo package in the locked workspace graph, every npm package
in `frontend/package-lock.json` (including frontend build dependencies), and all
eight catalog-managed runtime and model components. Managed downloads are not
embedded in the application files, their SBOM records contain the catalog URL,
SHA-256 digest, license, supplier, component kind, and source attribution. The
SBOM labels Cargo, npm, and managed components so consumers can separate the
ecosystems.

The workflow creates `SHA256SUMS` from the sorted file names, verifies every
digest, and rejects missing or extra files. On GitHub-hosted non-PR runs, the
isolated `attest-assets` job creates GitHub build-provenance attestations for
every staged file. Only that job can request an OIDC token and write
attestations. The tag job waits for the attestations, downloads the verified
directory, and checks it again before upload. Do not upload application assets
by hand.

## Verify

Confirm that the workflow is green. The GitHub Release must contain one Debian
package, one RPM, one AppImage, `echo-desktop`, `echo-desktop.cdx.json`, the
MIT license, `THIRD_PARTY.md`, and `SHA256SUMS`.

```sh
gh run list --workflow release.yml --limit 5
gh release view vX.Y.Z
```

Download the assets into an empty directory. Verify the checksums and visible
versions:

```sh
release_dir=$(mktemp -d)
gh release download vX.Y.Z --dir "$release_dir"
(cd "$release_dir" && sha256sum --check --strict SHA256SUMS)
for asset in $(awk '{print $2}' "$release_dir/SHA256SUMS"); do
  gh attestation verify "$release_dir/$asset" \
    --repo ddv1982/echo \
    --signer-workflow ddv1982/echo/.github/workflows/release.yml
done
dpkg-deb -f "$release_dir"/echo_*.deb Package
dpkg-deb -f "$release_dir"/echo_*.deb Version
chmod +x "$release_dir/echo-desktop"
"$release_dir/echo-desktop" --version
chmod +x "$release_dir"/*.AppImage
APPIMAGE_EXTRACT_AND_RUN=1 "$release_dir"/*.AppImage --version
```

`Package` must be `echo`. Do not GPG-sign application GitHub assets; those
files stay on `SHA256SUMS` and GitHub attestations.

## APT repository

APT is an operator path on top of the existing tag policy. It does not replace
annotated tags, main-only tags, SHA256SUMS, or attestations. GPG signs APT
metadata and the setup-package checksum only.

The Debian and RPM package name is `echo`. Users install with `apt install
echo`. Confirm a built package with:

```sh
dpkg-deb -f echo_*.deb Package
```

That field must be `echo`. The GitHub application publish directory stays the
same eight files. The APT setup package `echo-repository-setup` is not in that
directory.

APT publishes only from `v*` tags. Do not publish the repository from `main`
nightlies or `workflow_dispatch`.

### Operator setup

1. Generate one APT signing key (ed25519 or RSA 4096). Store private material
   only in Actions secrets.
2. Configure secrets `DEB_SIGNING_PRIVATE_KEY`,
   `DEB_SIGNING_KEY_FINGERPRINT`, and `DEB_SIGNING_KEY_PASSPHRASE`.
3. Configure repository variable `DEB_SIGNING_PUBLIC_KEY`.
4. Set Pages source to GitHub Actions (Settings → Pages).
5. Create the `github-pages` environment.

Users enable the repository with:

```sh
bash <(curl -fsSL https://ddv1982.github.io/echo/install-apt-repo.sh)
sudo apt update
sudo apt install echo
```

The bootstrap authenticates the setup package with the archive keyring.
Rotate the key by bumping `echo-repository-setup` beyond `1.0` so clients
replace `/usr/share/keyrings/echo-archive-keyring.pgp`.

## If a tag run fails

You can rerun the same GitHub Actions run while the tag still points to the same
commit. Do not move or repush the tag at another commit, and do not upload
artifacts from a dirty working tree. Fix the issue on `main`, repeat the package
gate, bump to the next patch version, and create a new tag. This keeps every
public tag tied to one reviewed commit and one reproducible workflow run.
Published releases, their assets, and Git tags are permanent history.
