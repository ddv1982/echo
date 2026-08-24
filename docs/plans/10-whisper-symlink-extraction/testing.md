# Verification plan

## Red before green

Add a synthetic tar whose order is:

1. `root/libtool.so -> libtool.so.1`
2. `root/libtool.so.1 -> libtool.so.1.2`
3. `root/libtool.so.1.2` as a regular file

The extraction plan maps these to flat payload destinations. Before the fix, the focused test must fail with the same target-not-extracted error reported by the user. After the fix, both `read_link()` values and the bytes read through the outer link must match.

## Boundary cases

- Direct link to a selected regular file.
- Reverse-order multi-link chain.
- Missing selected target.
- Target selected by the plan but absent from the archive.
- Target changed from the catalog value.
- Absolute and parent-traversing targets.
- Link cycle.
- Link terminal that is not a regular file.
- Flattened destination mismatch.
- Cancellation during post-scan validation, creation, or hashing.

Every failure must occur before activation. Tests inspect staging and the active pointer rather than relying only on the error text.

## Real artifact lever

`scripts/verify-whisper-runtime-archive.sh` downloads the pinned v1.9.2 archive unless a local path is supplied. An ignored Rust test reads the bytes through the fixture transport and calls the normal `Installer::ensure_spec()` path. It asserts:

- Outer size and SHA-256 match the catalog.
- Extraction, full payload verification, and activation succeed.
- The activation receipt contains the catalogued files.
- `libwhisper.so` and `libwhisper.so.1` retain their exact targets.
- Full managed verification succeeds after activation.

The script runs in CI so this bug cannot return while synthetic tests remain green.

## Adjacent guarantees

Run:

```sh
cargo test -p echo install::
./scripts/verify-first-run-readiness.sh
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release
```

Existing tests must continue covering successful installation, cancellation, interrupted resume, checksum failure, runtime-probe failure, repair, recovery, idempotent removal, and survival of external files.

## Release gates

- Version and changelog say v0.9.1.
- PR exact-head check and release workflows pass.
- Automated review has no unresolved actionable comments.
- The merge commit passes main check and release workflows.
- The annotated v0.9.1 tag targets that merge commit.
- Tag check and release workflows pass.
- Published binary, Debian package, and RPM package checksums match their release digests.
