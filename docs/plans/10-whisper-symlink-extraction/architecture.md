# Symlink graph architecture

## Problem

The checked-in inventory records exact immediate symlink targets and the terminal file's content hash. The runtime extractor writes regular files first, then processes links in archive order and requires each immediate target to resolve as a regular file. That condition rejects a valid link whose immediate target is another link stored later. The fix must remove the order dependency without weakening the closed allowlist, destination flattening, or activation boundary.

## Usage from the caller's view

The installer contract does not change:

```rust
extract_archive(&artifact, &payload, &extraction_plan(id).unwrap(), cancel)?;
verify_payload_cancellable(&payload, &expected, true, Some(cancel))?;
store.activate_with(spec, expected, &stage, operation)?;
```

The extractor preserves exact immediate targets:

```rust
assert_eq!(read_link(payload.join("libwhisper.so"))?, Path::new("libwhisper.so.1"));
assert_eq!(read_link(payload.join("libwhisper.so.1"))?, Path::new("libwhisper.so.1.9.2"));
```

The production artifact proof is one command:

```sh
./scripts/verify-whisper-runtime-archive.sh
```

## Shape

The new type remains private to `extract.rs`:

```rust
struct PendingSymlink {
    source: PathBuf,
    output_relative: PathBuf,
    target: PathBuf,
    expected_sha256: String,
}

fn validate_symlink_graph(
    selected: &BTreeMap<PathBuf, &ExtractFile>,
    regular_files: &BTreeSet<PathBuf>,
    symlinks: &BTreeMap<PathBuf, PendingSymlink>,
    cancel: &AtomicBool,
) -> Result<(), InstallError>;
```

`source` and graph edges use normalized archive paths. `output_relative` uses the flattened catalog destination. These are separate coordinate systems. For every edge, validation proves that resolving the raw immediate target beside `output_relative` reaches the selected target's catalog destination. The raw target is never rewritten.

`validate_symlink_graph()` walks from every link until it reaches a selected regular file. A per-walk set detects cycles. The selected entry bound makes the simple repeated walk preferable to shared visit-state machinery. The helper has no filesystem mutations.

`extract_tar()` remains the I/O shell. It streams and hashes regular files, gathers links, checks selected-member completeness, validates the graph, creates all exact links, then hashes all links. Unix permits creation before an immediate target exists. These transient links remain inside operation-owned staging, and no reader or active pointer can observe them. Graph validation occurs before link creation, and full payload verification occurs before activation.

## Module map

- `crates/echo/src/install/extract.rs` owns graph validation, link creation, hashing, and focused hostile fixtures.
- `crates/echo/src/install/tests.rs` owns the pinned-artifact full installer regression and lifecycle guarantees.
- `scripts/verify-whisper-runtime-archive.sh` downloads the pinned artifact and runs the installer regression.
- `installer.rs`, `mod.rs`, the catalog, receipts, cleanup, repair, and runtime selection keep their current interfaces.

## Synthesis decision

SOL scored 25 out of 25 and is the base. Its complete-graph validation plus create-all and hash-all passes keep the invariant inside two extractor functions. Luna scored 23 out of 25. Its normalized archive graph versus flattened destination distinction and production-shaped installer proof are grafted into the base. Luna's topological ordering and visit-state helper are rejected because private staging and final payload verification already make them unnecessary. G55 exceeded the bounded design scope and was interrupted before a durable candidate completed, so it is recorded as a dropout.

## Tradeoffs accepted

- We accept repeated walks over a symlink set bounded by the catalog entry limit in exchange for one small pure validator.
- We accept transient dangling links inside private staging in exchange for avoiding ordering machinery. The graph is already closed, and every link is hashed only after all links exist.
- We keep existing string error categories instead of adding public variants because callers only require a safe pre-activation failure.
- We add a 9.5 MiB pinned upstream download to the focused CI verifier in exchange for proving the exact artifact users install.

## Alternatives considered

- Topological materialization is safe but adds visit state and an order result that no caller consumes.
- Retrying links until filesystem progress stops hides whether the cause is a missing node, cycle, or destination mismatch.
- Reordering inventory fixes only this archive and preserves the false order contract.
- Rewriting links to terminal files breaks exact immediate-target receipts.
- `tar::Archive::unpack()` violates the closed selected-member and flattened-destination contract.

## Open risks

- Concurrent mutation of operation-owned staging remains outside the existing threat model. The installer lock and private operation path are the controlling boundary.
- The upstream release URL could become unavailable. Outer size and SHA-256 still prevent changed bytes from entering the test or product.

## Next implementation step

Add the reverse-order chain fixture and confirm it fails before introducing `PendingSymlink` and `validate_symlink_graph()`.
