# CI and release pipeline simplification

Goal: cut duplication in `.github/workflows/check.yml` and `release.yml`
without dropping any check. Required check names (`check`, `release-policy`,
`linux-packages`, `release-assets`) stay unchanged because branch protection
references them.

- [x] Restore green audits: update rustls, rustls-webpki, webpki-roots (cargo) and undici (npm); `cargo audit` and `npm audit` pass locally
- [x] Fold in open Dependabot updates (#195-#205, except #199) with attribution; workspace builds and tests pass. #199 (@tauri-apps/api 2.12) stays separate: `cargo tauri build` rejects it against the tauri 2.11 crate
- [x] Extend `verify-workflow-pinning.py` to scan `.github/actions/*/action.yml` for unpinned refs; self-test shows a floating ref there is rejected
- [x] Add composite action `.github/actions/setup-build` (apt deps, Rust toolchain, rust-cache, optional Node + `npm ci`) and use it in frontend, rust, assets, linux-packages, appimage
- [x] Point `verify-settings-ux.sh` dependency greps at the composite action
- [x] Define `ARTIFACT_STEM` once at workflow level in release.yml
- [x] Replace the four inline workspace-version one-liners with `scripts/workspace-version.py`
- [x] Run the pinning verifier and changelog self-test once per PR instead of in both `policy` and `release-policy`
- [x] Local: pinning verifier, ruff, release-policy scripts, actionlint pass
- [ ] PR CI green (check + release-policy)
- [ ] `workflow_dispatch` run of release.yml on the branch: linux-packages, appimage, release-assets, attest-assets green

Out of scope (recommended follow-ups): rewriting the hand-rolled YAML parser in
`verify-workflow-pinning.py`, moving the tag-only APT signing block into a script
(only provable on a real tag).
