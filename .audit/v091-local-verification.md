# v0.9.1 local verification

## Red regression

Commit `17ebe85` adds `install::extract::tests::chained_symlinks_do_not_depend_on_archive_order` before the production fix. Running the exact test at that commit failed with:

```text
UnsafeArchive("symlink target was not extracted for root/libtool.so")
```

Commit `c9decf0` adds graph validation and makes the same test pass.

## Pinned production artifact

`./scripts/verify-whisper-runtime-archive.sh` downloaded `whisper-bin-ubuntu-x64.tar.gz` from the v1.9.2 release. `Installer::ensure_component()` admitted the compiled size and SHA-256, extracted and verified the payload, activated an immutable generation, preserved both immediate `libwhisper` targets, and passed `ManagedStore::verify()`.

Local macOS runs use `AcceptProbe` because the archive is a Linux binary. The same test selects `CommandRuntimeProbe` on Linux x86_64, and `.github/workflows/check.yml` runs it on Ubuntu. That runtime execution remains open until exact-head CI passes.

## Final local gates

The final branch after interrogation changes passed:

- `cargo test -p echo install::` with 23 passing tests and the pinned-artifact test invoked separately.
- `cargo test --workspace` with 138 of 139 Echo tests passing and the network-backed test ignored in the ordinary suite, plus all non-ignored workspace tests.
- `./scripts/verify-whisper-runtime-archive.sh` against a fresh download.
- `./scripts/verify-first-run-readiness.sh`.
- `cargo clippy --workspace --all-targets -- -D warnings`.
- `cargo build --release` and `target/release/echo-desktop --version`, which printed `echo-desktop 0.9.1`.
- `./target/release/xtask`.
- 91 frontend tests under Node 22, frontend lint, typecheck, and production build.
- `actionlint .github/workflows/check.yml`.
- `shellcheck scripts/verify-whisper-runtime-archive.sh`.
- `git diff --check`.
