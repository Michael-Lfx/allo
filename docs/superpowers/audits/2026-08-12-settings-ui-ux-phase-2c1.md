# Settings UI/UX Phase 2C.1 Audit

## Baseline

- Branch: `feat/interaction-style-optimization`.
- Rebase baseline: `origin/main...HEAD`.
- Existing WIP changes were preserved; no reset, commit, or push was performed.
- The shared market card used `h-full` with padding and borders inside a
  stretched Grid row. Because the project does not apply a global box-sizing
  reset, the card exceeded its track and overlapped the next row.
- Skills, Preset Packages, and MCP still calculated installed state at the page
  level but did not pass it into the shared action component after rebase.

## Implemented in this batch

- Added shared `MarketCardGrid` and `MarketCardShell`.
- Removed the conflicting card `h-full` sizing.
- Added `MarketActionState` completion labels and consumer resolvers.
- Synchronized card and detail-drawer pending/completed state.
- Restored Skills, Preset, and MCP completion-state wiring.
- Added localized installed, added, and imported labels.
- Removed the duplicate details entry from the card more menu.

## Automated evidence

- Focused market, layout, view-model, preset, and action-state tests pass.
- Direct TypeScript check reaches only existing baseline errors in reasoning
  effort types and the MediaPipe video dependency.
- Full `bun run typecheck` is currently blocked by the restricted Bun script
  invocation (`Operation not permitted`); direct `tsc` was used instead.
- Browser geometry, theme matrix, Web development build, and Windows desktop
  acceptance remain to be run after the UI is launched.

## Remaining acceptance

Capture real `getBoundingClientRect()` measurements for all four markets,
verify no horizontal overflow, inspect completed states after refresh, and run
classic light plus all built-in light/dark themes. Keep the known reasoning
effort, MediaPipe, and other unrelated baseline failures separate.

Follow-up evidence is recorded in
[Phase 2D visual contract audit](2026-08-12-home-settings-visual-contract-phase-2d.md).
