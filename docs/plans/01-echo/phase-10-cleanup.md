# Phase 10. Cleanup pass

Back to [overview](./overview.md).

## Goal

Optional local rewrite that strips fillers and adds punctuation. This is last because an insert that does not work makes a prettier sentence irrelevant.

## Changes

`crates/echo-core/src/cleanup.rs` defines `Cleanup` as a pure function over `Transcript` plus a prompt.

`crates/echo/src/cleanup/ollama.rs` or a stdin child process. One adapter. Default off.

`crates/echo/src/cleanup/rules.rs` is a tiny deterministic fallback. Drop standalone um / uh / like, capitalize the first letter, ensure ending punctuation. Ships on so a machine with no LLM still sounds closer to the video.

Do not call a cloud API.

## Data structures

`Cleanup` is `fn apply(&self, raw: &str, dict: &Dictionary) -> Result<Rewrite>`.

`CleanupMode` is `Off | Rules | LocalModel { model }`. Illegal combinations (cloud URLs) do not parse.

## Verification

Static. Rules tests on a spoken ramble fixture. "um so like can we uh move the button" becomes a clean sentence. Dictionary hits still apply after cleanup.

Runtime. With `CleanupMode::Off`, output equals the engine raw plus dictionary. With `Rules`, the fixture matches golden text. With `LocalModel`, an ignored test runs only if the binary is on `PATH` and compares fillers-removed, not exact wording. Linux can prove Off and Rules. The model path is best-effort here.
