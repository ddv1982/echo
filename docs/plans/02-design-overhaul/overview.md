# Design overhaul: aura-inspired minimal Echo

## Context

Echo's desktop window works, but its visual language fights the product. The current design layers a blue-tinted ten-step surface palette, radial gradient textures, card shadows, glow shadows, icon chips on every heading, and a decorative hero orb with a fake waveform. The result reads busy, not calm, for a utility whose whole job is one shortcut and clean text. The declared typeface (Inter, `frontend/src/styles/tokens.css` line 31) is never bundled, so the app actually renders in whatever system fallback matches, and the CSP (`src-tauri/tauri.conf.json`, `font-src 'self'`) means a web font could never load anyway. The user asked for a complete overhaul toward a clean, sleek, minimal design with great UX, citing aura.build as the reference.

The aura.build design language, extracted from live CSS analysis (educlopez/design-bites `design-mds/aura.build/DESIGN.md`) and Aura's own published System 01, is concrete:

- A grayscale palette with zero hue. Hierarchy comes from lightness steps and opacity, never decorative color.
- Flat surfaces. No box shadows on components. Depth via background lightness steps and hairline 1px borders.
- Inter as the only typeface, with OpenType features enabled (`"calt"`, `"rlig"`, `"salt"`, `"ss01"`, `"ss02"`), medium (500) display weight, tight tracking.
- Small all-caps labels with wide tracking (about 0.15em) as a "terminal readout" style for section labels and table headers.
- Double-ring focus states (2px inner ring in the surface color, 4px outer dark ring) instead of glows.
- Restrained radii, roughly 8px to 12px.
- Line-style minimal icons, generous section rhythm, border-only separators.

Echo adapts this with one deliberate deviation. A dictation app has a live-recording state that must be unmistakable, so we keep exactly one functional accent, the recording red, used only when audio is being captured and for destructive actions. Everything else goes grayscale.

## Scope

**Included**

- All four stylesheets in `frontend/src/styles/` (tokens, base, shell, views).
- Markup and component changes inside `frontend/src/App.tsx` where the redesign removes or reshapes UI (hero, health grid, theme control, headings).
- Bundling Inter properly (`frontend/package.json`, `frontend/src/styles/index.css`).
- Test updates in `frontend/src/App.test.tsx` where markup changes break selectors.
- The X11 HUD capsule palette in `crates/echo/src/ui/hud.rs`, so both surfaces speak the same language.

**Excluded**

- New features: real audio level meters, global shortcut registration, editing history entries, new views.
- Backend behavior, session state machine, CLI subcommands.
- Tailwind, CSS-in-JS, or any component library. Four plain CSS files are the right size for this app; adding a framework would grow the surface, not shrink it (the laziness-protocol principle).
- Window chrome changes beyond what the restyle needs.

## Constraints

- The Tauri webview on Linux is WebKitGTK. Verify `backdrop-filter` and OpenType feature rendering there, not just in Chrome.
- CSP `font-src 'self' data:` requires fonts bundled into the Vite build. `@fontsource-variable/inter` satisfies this; no network fetches.
- `npm run build` runs `tsc --noEmit` first; vitest with jsdom drives `App.test.tsx`. Both must stay green per phase.
- `prefers-reduced-motion` support exists today and must survive every phase.
- The HUD is raw X11 with u32 hex colors; it cannot read CSS tokens. Its palette is duplicated by design and the phase notes the pairing.
- Keep the existing HSL triplet token convention (`--token: H S% L%` consumed as `hsl(var(--token))`). Changing the convention would touch every rule for zero user-visible gain.

## Alternatives

Three directions were considered (the exhaust-the-design-space principle):

1. **Pure aura clone, zero hue anywhere.** Grayscale even for the recording state, relying on the pulsing dot and label alone. Rejected. Recording is a safety-relevant state (the mic is live); color is the strongest signal we have and this is the one place decoration and function align.
2. **Grayscale plus one functional accent (chosen).** The full aura language, with recording red as the single hue, applied only to live-capture and destructive UI. Success and warning shrink to tiny status-dot colors. This keeps the minimal feel and keeps the mic state unmistakable.
3. **Flatten in place, keep the cyan brand.** Cheapest diff, but it fails the brief. The cyan-everywhere accent (nav, badges, icons, orb, borders) is the main source of visual noise, and keeping it means the app never reads as a different design.

## Applicable skills

The implementer must invoke these by name:

- **how** over the frontend and the HUD module before the first change to each.
- **visual-parity** thinking is not the goal here (this is a redesign, not a port), but each phase's before/after screenshots follow its discipline.
- **control-ui** (from `cursor-team-kit`) for runtime verification of every frontend phase.
- **unslop** over every prose surface, including this plan's revisions and PR descriptions.
- **deslop** (`/deslop`) over each diff before commit.
- **no-comments** (`/no-comments`) before review.

## Phases

Ordered so subtraction lands first (the subtract-before-you-add principle), then the token and typography scaffold every later phase builds on (the foundational-thinking principle), then surfaces, then motion, then the HUD.

1. [Phase 1: strip the decorative hero](phase-1-strip-hero.md)
2. [Phase 2: strip global decoration](phase-2-strip-decoration.md)
3. [Phase 3: ship Inter for real](phase-3-ship-inter.md)
4. [Phase 4: grayscale token system](phase-4-grayscale-tokens.md)
5. [Phase 5: controls and focus rings](phase-5-controls-and-focus.md)
6. [Phase 6: shell redesign](phase-6-shell.md)
7. [Phase 7: home view redesign](phase-7-home.md)
8. [Phase 8: history and dictionary views](phase-8-history-dictionary.md)
9. [Phase 9: settings view](phase-9-settings.md)
10. [Phase 10: motion pass](phase-10-motion.md)
11. [Phase 11: HUD palette alignment](phase-11-hud.md)

Verification detail per phase lives in [testing.md](testing.md).

## Verification

Project-level commands, run per phase and at completion:

```sh
npm run build --prefix frontend
npm run test --prefix frontend
npm run lint --prefix frontend
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

Runtime verification per phase is in [testing.md](testing.md). The frontend is exercised through the running UI via the control-ui skill. The X11 HUD has no control skill; that gap is flagged in phase 11 and testing.md with a manual fallback (`echo-desktop --hud-demo` under Xvfb plus a screenshot).

## Implementation guidance

- Branches follow `cursor/<descriptive-name>-3dc1`.
- One phase per PR, in order. Each phase is independently shippable; the app must look intentional after every merge, though phases 1 and 2 are transitional by design (flatter but still cyan) per the outcome-oriented-execution principle.
- Apply the **how** skill before touching an unfamiliar file, `/deslop` before each commit, the **unslop** skill on PR prose, and Cursor's built-in **babysit** skill after opening each PR.
- No new abstractions. No CSS preprocessor, no utility framework, no component split beyond what `App.tsx` already has unless a phase names it.
- Do not carry compatibility styles between phases. When a phase replaces a pattern, delete the old rules in the same diff (the migrate-callers-then-delete-legacy-apis principle).
- Take a before and an after screenshot at 920x680 in both themes for every visual phase and attach them to the PR.
