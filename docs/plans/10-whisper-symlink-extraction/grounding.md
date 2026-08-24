# Whisper runtime symlink extraction grounding

## Definition of done

The pinned Whisper 1.9.2 Ubuntu x64 archive installs from verified download through immutable activation. Its chained library symlinks retain their exact immediate targets and resolve to the catalogued regular files. Archive member order cannot affect the result. Missing, escaping, changed, cyclic, unselected, or destination-mismatched targets fail inside staging, and no failed attempt changes the active generation.

## Reproduction

Echo pins `whisper-bin-ubuntu-x64.tar.gz` from the upstream v1.9.2 release at 9,497,583 bytes with SHA-256 `46811a3ecf584307480a220b9ef5ff81b7b22dc41577cbc274ce3afc61f753b1`. A fresh download matched that digest.

The archive stores this chain in dependency-reversed order:

```text
libwhisper.so -> libwhisper.so.1 -> libwhisper.so.1.9.2
```

`libwhisper.so` appears first, `libwhisper.so.1` later, and the regular file later still. `extract_tar()` extracts regular files, queues symlinks, then processes queued links in archive order. Before creating a link, it requires the immediate target path to be a regular file. The first link therefore fails because its target is another not-yet-created symlink.

## Existing flow

1. `download_verified()` admits only the pinned outer byte count and SHA-256.
2. `Installer::ensure_spec()` creates operation-owned staging.
3. `extract_archive()` selects only catalogued members and validates paths, types, sizes, modes, and content hashes.
4. `verify_payload_cancellable()` verifies every installed file and link.
5. The runtime probe runs before activation.
6. `ManagedStore::activate_with()` renames staging to an immutable release and changes the active pointer last.
7. Any error removes staging and preserves the verified resumable download.

## Constraints

- Keep the selected-member allowlist and flattened payload layout.
- Preserve exact immediate link targets because receipts and later verification compare them.
- Reject absolute targets and parent traversal.
- Reject missing selected targets and cycles before activation.
- Do not make correctness depend on archive or inventory order.
- Do not use `tar::Archive::unpack()`. Echo intentionally extracts a closed subset into catalogued destinations.
- A transient link inside operation-owned staging is not user-visible, but the final graph must be closed before payload verification.
- Removal must continue deleting links themselves through `symlink_metadata`, never following them into external paths.

## Research evidence

- [whisper.cpp v1.9.2](https://github.com/ggml-org/whisper.cpp/releases/tag/v1.9.2) publishes the pinned Ubuntu x64 archive.
- [The Rust `tar` crate security contract](https://docs.rs/tar/latest/tar/index.html) treats paths and symlink targets as extraction-boundary data and documents its concurrent-mutation threat boundary.
- `scripts/generate-managed-inventory.py::resolve_symlink()` already walks complete chains, rejects cycles, and hashes the terminal regular file. Runtime extraction is the inconsistent side.
- Ref documentation search returned no indexed result for this crate. The pinned local crate source and official docs.rs source were inspected directly instead.

## Scope and rigor

The likely production change is one extractor helper plus the existing symlink materialization loop. Tests cover a synthetic reversed chain, cycles, missing and escaping targets, destination mapping, cancellation, failed activation cleanup, repair, and removal. A rerunnable verifier downloads the 9.5 MiB pinned archive and drives the real installer path. This is high rigor because extraction accepts network bytes and precedes activation, but the change remains local and reversible.
