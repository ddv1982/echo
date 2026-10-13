# Echo UI and dependency refresh

## Checklist

- [x] Record Exa/Ref research and inspect baseline screenshots at desktop, compact and narrow widths.
- [x] Apply restrained shared styling and verify navigation, focus, contrast, reduced motion and existing workflows with rendered evidence and the repository gate.
- [x] Obtain independent Flow review of the UI slice.
- [x] Upgrade project Rust/MSRV and all five CI toolchains to 1.92.0, verify local tools and maintained build documentation.
- [x] Adopt Node 24.21.0 LTS in the root version pin, frontend engines, all four CI setups and build docs; verify strict-engine installation.
- [x] Resolve ui-polish.R13-01, ui-polish.R13-02 and ui-polish.R13-03 with failing/passing acceptance and refreshed screenshots.
- [x] Consolidate compatible dependencies from PRs #180–#192 and record compatibility and cleanup disposition.
- [x] Run final repository validation, obtain independent review and deliver the completed Flow result.

## Design assessment and direction

Echo already has useful semantic tokens, light/dark themes, accessible labels,
locally bundled Inter and a task-based four-view structure. Refine these into
a compact desktop utility. The default native window is 920×680, minimum
760×560. Current CSS hides navigation labels below 960px, uses a 28–40px Home
heading and heavily tracked uppercase metadata, and imports responsive rules
before later rules that override compact hero padding.

Use visible navigation labels, a smaller heading scale, quieter metadata,
consistent spacing and controls, distinct neutral surfaces and deliberate
keyboard focus. Keep recording/errors semantically colored and textual.
Preserve existing behavior and progressive Settings disclosures.

## Research

Exa search and research run `agent_run_e0a35b1db71b46c68bfc6523b9958bef`
retrieved primary GNOME and WCAG guidance:

- [GNOME typography](https://developer.gnome.org/hig/guidelines/typography.html):
  use few sizes/weights, avoid all-caps labels, use relative sizing. Keep the
  existing bundled font, reduce oversized Home typography and tracked captions.
- [GNOME navigation](https://developer.gnome.org/hig/guidelines/navigation.html):
  simple, flat navigation and digestible views. Keep all four destinations
  recognizable even in compact windows.
- [GNOME adaptiveness](https://developer.gnome.org/hig/guidelines/adaptive.html):
  resize and reflow without losing functionality. Verify native default/minimum,
  wider desktop and narrow browser preview.
- [GNOME UI styling](https://developer.gnome.org/hig/guidelines/ui-styling.html):
  restrained styling, meaningful grouping and minimal decorative motion.
- [WCAG non-text contrast](https://www.w3.org/WAI/WCAG22/Understanding/non-text-contrast):
  inspect focus and essential controls as well as text contrast.
- [WCAG status messages](https://www.w3.org/WAI/WCAG22/Understanding/status-messages):
  retain textual status and existing live-region semantics without moving focus.
- [WCAG reflow](https://www.w3.org/WAI/WCAG22/Understanding/reflow.html) and
  [text spacing](https://www.w3.org/WAI/WCAG22/Understanding/text-spacing.html):
  wrap long content, avoid clipped controls and horizontal page scrolling.

Ref searched React accessibility guidance and read
[React select labels](https://github.com/reactjs/react.dev/blob/main/src/content/reference/react-dom/components/select.md?plain=1#L110):
labels must remain associated with controls, with `useId` when unique IDs are
needed. This refresh retains existing semantic controls and accessible names.

## Verification contract

Use existing Playwright/preview tooling; capture light/dark screenshots at
1280×900, 920×680, 760×560 and 390×844, including representative empty/busy/error
states. New navigation acceptance must fail before the styling fix. Inspect
actual images, not only screenshot file existence.

Canonical Linux gate assembled from maintained `.github/workflows/check.yml`:

```sh
npm run build --prefix frontend && npm run lint --prefix frontend && npm run test --prefix frontend && npm run ipc:check --prefix frontend && ./scripts/verify-settings-ux.sh && cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && timeout --signal=TERM --kill-after=30s 20m xvfb-run -a cargo test --workspace && python3 scripts/verify-workflow-pinning.py --self-test && python3 scripts/verify-workflow-pinning.py
```

Final host target: Linux, Node 24.21.0 LTS, Rust 1.92.0. CI uses the root
`.nvmrc` pin. Historical UI checks used Rust 1.89.0. Browser preview
evidence does not establish physical dictation or native WebKitGTK behavior.
Existing Settings edge tests passed 2/2 after installing missing Chromium.

## Dependency scope

Inventory: #180 clap 4.6.7, #181 jsdom 30.1.0, #182 rustix 1.1.5,
#183 vite 8.3.0, #184 vitest 5.0.1, #185 oxlint 1.83.0,
#186 @types/node 26.6.1, #187 glib 0.22.10, #188 tauri 2.11.6,
#189 single-instance 2.4.5, #190 upload-artifact 7.0.2,
#191 download-artifact 8.0.2, #192 setup-node 7.1.0.

Targeted updates only, retain locks and full-SHA action pins. Assess glib using
actual MSRV/resolver/compiler evidence; parallel versions already appear in its
PR lockfile, so do not infer incompatibility from duplicated GLib alone.

Scope revision approved by the user: upgrade Rust to 1.92 as well as the
intended dependencies. The original Flow session was deferred and replaced by
one integrated feature accepting the existing edits against original commit
`cb712df9ca728d98b176d860fc6edad6dbd6095c`. Pin Rust 1.92.0 with rustfmt/clippy
in `rust-toolchain.toml`, update the project-local rustup override, workspace
MSRV, CI and maintained build docs. Keep the global rustup default unchanged.
Upgrade direct GLib to 0.22.10 and verify the existing GNOME GVariant contracts.

Authoritative `cargo info glib@0.22.10` reports Rust 1.92. Ref and versioned
[GLib manifest](https://docs.rs/crate/glib/0.22.10/source/Cargo.toml) research
confirms this requirement and that Echo's used GVariant APIs remain available.
Keep the intended Tauri 2.11.6/single-instance 2.4.5 versions. Accept compatible
frontend patches already resolved above the original PR target versions.

Second scope revision requested by the user adds R7: a consistent Node 24 LTS
policy. Root `.nvmrc` pins 24.21.0, frontend `engines.node` is `^24.21.0`, all
four setup-node invocations read that file, and build docs describe the same
supported line. `npm ci --prefix frontend --engine-strict` is a required check.

Exa research `agent_run_b5c9449f16a54f1d9563a3e2ff332c3e` and Ref GitHub docs:

- [Node release status](https://nodejs.org/en/about/previous-releases): on
  October 13, 2026, Node 24 is LTS and Node 26 is Current. Production guidance
  recommends LTS. Node 26's LTS transition is planned for October 28 in the
  [release schedule](https://github.com/nodejs/Release/blob/main/schedule.json).
- [GitHub Actions migration](https://github.blog/changelog/2026-09-23-node-20-is-no-longer-available-in-github-actions/):
  the JavaScript-action runtime moved from Node 20 to Node 24, not Node 26.
- [Ref: project Node selection](https://docs.github.com/en/actions/tutorials/build-and-test-code/nodejs#specifying-the-nodejs-version):
  setup-node selects the project build runtime independently of action runtime.
- Actual npm engine metadata: jsdom 30.1.2 supports `^22.22.2 || ^24.15.0 ||
  >=26.0.0`; Vite 8.3.4 supports `^20.19.0 || >=22.12.0`; Vitest 5.0.3 supports
  `^22.12.0 || ^24.0.0 || >=26.0.0`. Supporting Node 22 as an alternative does
  not force us to remain on it. Keep these intended dependency upgrades.

Revisit Node 26 after its LTS transition and separate compatibility validation;
this delivery targets the currently supported Node 24 LTS line.

Closing PRs or deleting remote branches requires updates to exist upstream.
This request authorizes implementation, not commit/push/merge. Record exact
cleanup readiness and retain unsuperseded PRs until publication.

## Results

Baseline: 40 screenshots captured in `.audit/ui-design/before/` across the four
views and recording, light/dark and all four viewport sizes. Inspected the
920px light Home, 1280px dark Settings and 390px light Dictionary images.
Navigation-label acceptance failed at 920px (`Home` span hidden); screenshot
matrix and reduced-motion checks passed. Several compact Settings/setup
actions visibly use browser-default button styling. Fix their shared styling
within the existing control system.

UI implementation: semantic neutral surfaces, amber selection/focus cues,
smaller headings, quieter captions, compact horizontal hero, two-column Home
content at native widths, labeled compact navigation, styled compact actions
and explicit control borders. Responsive rules now follow general view rules.
Long transcripts and dictionary values wrap. No React behavior was changed.

UI validation: the design suite passed 6/6, including compact navigation from
320px to 920px, keyboard activation/focus, reduced motion, light/dark text ≥4.5:1
and essential border/focus tokens ≥3:1 across normal/hover surfaces, and narrow
empty/processing/error screenshots. A dark hover border initially measured
2.77:1 and was strengthened before passing. After images are in
`.audit/ui-design/after/`; inspected matching Home/Settings/Dictionary and dark
recording images plus narrow error/processing/empty History and keyboard focus.

The complete canonical gate passed: 330 frontend tests, 21 browser tests,
typecheck/build/lint, IPC generation check, Settings UX verification, Rust
fmt/clippy/workspace tests and workflow pin self-test/check. Existing opt-in
hardware/model tests remain ignored by their maintained defaults. Physical
microphone/dictation and native WebKitGTK visual appearance were not exercised.

Independent UI review passed at Flow revision 14, inspecting 15 screenshots.
That review recorded three non-blocking advisories: `ui-polish.R13-01` (compact Dictionary
column proportions), `ui-polish.R13-02` (compact non-primary button height
specificity), and `ui-polish.R13-03` (contrast test theme init-script ordering).
The supplied run's light/dark images were verified correct. Flow refused an
automatic reopening of the passed slice, requiring explicit user direction;
the approved replacement feature now includes their targeted fixes and fresh
integrated review. Earlier passing evidence remains historical until rerun.

## Integrated results (Rust 1.92 / Node 24 LTS)

- Local readback: rustc/cargo 1.92.0, rustfmt 1.8.0, clippy 0.1.92,
  Node 24.21.0 and npm 11.19.0. The project-local Rust override is now 1.92.0;
  global defaults were not changed. All five Rust CI pins and four Node
  version-file references were read back from maintained workflows.
- GLib 0.22.10 resolved and compiled. The existing GNOME suite passed
  12 tests (two existing opt-in tests ignored); isolated X11 and portal
  shortcut lifecycle tests each passed. Direct GLib is 0.22.10; Tauri's GTK3
  stack still transitively uses 0.18.5. The upgrade required no GVariant API
  or application behavior changes.
- `ui-polish.R13-01`: observed field width differences of 51px at 920px and
  29px at 760px before repair. Explicit three-column compact layout now passes
  equal-width, placement and alignment checks. Inspected refreshed
  `compact-920-dictionary-controls.png` and `compact-760-dictionary-controls.png`.
- `ui-polish.R13-02`: the copy action's compact min-height was 1.6px too tall.
  Zero-specificity `:where()` exclusion preserves primary styling while allowing
  the compact modifier to apply; the same computed-height acceptance now passes.
- `ui-polish.R13-03`: each contrast theme is now a separate isolated test and
  asserts the actual applied theme. The screenshot matrix does likewise.
  Ten design tests passed, including both isolated themes and all repairs.
- Strict-engine clean installation passed on Node 24.21.0: 115 packages
  installed, zero npm vulnerabilities reported.
- The canonical integrated gate passed on the new toolchains: 330 frontend
  unit tests, the complete browser suite, build/typecheck/lint, IPC check,
  Settings UX verification, Rust fmt/clippy/workspace tests and workflow pins.
  Final independent review passed and Flow was closed as completed. The final
  run passed 25 browser tests in addition to the 330 frontend unit tests.

### Dependency / PR disposition

All 13 requested PR updates are represented in the local worktree. All PRs and
branches remain open until the superseding changes are published and integrated
upstream. No commits, pushes, merges or remote deletions have been performed.

| PR | Dependency | Verified local version | Cleanup condition |
| --- | --- | --- | --- |
| #180 | clap | 4.6.7 | Superseded after upstream integration |
| #181 | jsdom | 30.1.2 | Superseded after upstream integration |
| #182 | rustix | 1.1.5 | Superseded after upstream integration |
| #183 | Vite | 8.3.4 | Superseded after upstream integration |
| #184 | Vitest | 5.0.3 | Superseded after upstream integration |
| #185 | oxlint | 1.83.0 | Superseded after upstream integration |
| #186 | @types/node | 26.6.5 | Superseded after upstream integration |
| #187 | direct GLib | 0.22.10 | Superseded after upstream integration |
| #188 | Tauri | 2.11.6 | Superseded after upstream integration |
| #189 | single-instance | 2.4.5 | Superseded after upstream integration |
| #190 | upload-artifact | 7.0.2 | Superseded after upstream integration |
| #191 | download-artifact | 8.0.2 | Superseded after upstream integration |
| #192 | setup-node | 7.1.0 | Superseded after upstream integration |

All 18 action references were updated across both workflows. GitHub tag refs
confirmed setup-node SHA `949feb2413d6458794dcd2491c4babbbce0c15c1`,
upload-artifact SHA `cf430e030ddbb5b0abf93d22962f4752f3646cd9`, and
download-artifact SHA `9000827ccba6bdab643e8b6fd33ac0654aef8333`.

### Audit and verification limits

`cargo audit` exited successfully with seven allowed warnings: six unmaintained
transitive crates (`proc-macro-error` and five `unic-*` crates), plus
RUSTSEC-2024-0429 for transitive GLib 0.18.5's `VariantStrIter`. Updating Echo's
direct GLib does not remove GTK3's old GLib. Do not describe this as a clean
Rust audit or invent a dependency-only way to remove the native GTK3 stack.

Physical microphone/dictation, native WebKitGTK visual appearance and hosted
CI/package execution remain unverified. Browser evidence covers the preview
implementation, rendered styling and interaction contracts; native automated
tests cover the exercised Rust/shortcut contracts. Hardware/model opt-in tests
retain their existing defaults. Existing user-owned untracked plans remain
untouched.

## Final review and delivery

Independent review `review:126e07ad-eaaa-43d2-97af-fc89fbc201da` passed at Flow
revision 18, inspecting all 21 original-base changed files and 18 actual images.
R1–R7 and all three prior UI advisory fixes were verified. The session was
closed and archived as completed at revision 19. This checklist/result update
was administrative, after accepted review and archive; product files were not
changed afterward.

Two non-blocking findings remain:

- `integrated-ui-dependencies.R17-01`: the stalled-response-body test failed
  once with empty rather than saved partial bytes. Three isolated runs and
  the unchanged full canonical gate subsequently passed. Intermittency was
  disclosed, not repaired or hidden by relaxing the test.
- `integrated-ui-dependencies.R17-02`: the seven allowed Rust audit warnings
  above remain, including transitive GLib 0.18.5 unsoundness. The audit is not
  clean.

No checklist items remain unchecked. PR/branch cleanup is explicitly conditional
on publication and upstream integration; physical/native visual and hosted CI
limits remain as documented above.
