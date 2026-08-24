# v0.9.1 interrogation verdict

## Intent

Fix managed Whisper installation for the pinned 1.9.2 archive without weakening selected extraction, path safety, hashes, cancellation, staging cleanup, atomic activation, repair, recovery, or removal. Add an exact-artifact Linux CI proof and release v0.9.1.

## Reviewers

- `gpt-5.6-sol`: no findings.
- `gpt-5.5`: no actionable findings.
- `gpt-5.4`: no findings.
- `gpt-5.6-luna`: one warning about verification claims exceeding checked-in tests.

## Act on

No code blocker reached this category.

## Consider

Luna found that the testing plan claimed phase-specific cancellation and active-pointer inspection for every hostile graph case. The implementation contains cancellation checks, and existing installer tests cover cancellation and activation safety, but no deterministic hook flips cancellation inside each new post-scan loop. The resolution adds cheap changed and escaping target cases and narrows the plan to the evidence. Production code does not gain test-only hooks.

## Noted

- The graph walk repeats bounded work. Current catalog entry limits keep it small.
- The CI proof depends on upstream release availability. Size and SHA-256 admission prevent changed bytes from passing.

## Dismissed

- Creating links before immediate targets is safe here. Graph closure is proven first, staging is operation-owned, and hashing waits until every link exists.
- Cleanup does not follow links. Managed removal uses `symlink_metadata` and deletes only catalogued entries.
- The real-artifact proof does not bypass download admission. `Installer::ensure_component()` rechecks the compiled size and SHA-256 before extraction.

## Agreement map

All four reviewers agreed that the order-independent graph fix preserves the product's safety boundaries. The only disagreement concerned the precision of the written test claims, not runtime behavior.
