# Home and Settings UI/UX Phase 2D Audit

Follow-up interaction evidence: [Workspace picker and Preset editor rebase
follow-up](2026-08-12-uiux-rebase-popover-preset-fixes.md).

## Baseline

- Branch: `feat/interaction-style-optimization`.
- Existing Phase 2C.1 and Mini App visibility WIP was preserved.
- No reset, commit, push, route, backend, or persistence change was performed.
- The initial focused audit had stale assertions for Home send routing, selected
  tag tokens, shared Settings navigation, and sidebar scroll classes.
- Real deviations were a hard-coded Preset warning color, a Settings tab width
  transition, an English-only narrow navigation label, and an unused decorative
  Home poster implementation.

## Implemented

- Preset warning presentation now consumes `--warning-6`.
- Settings tabs animate transform only and retain Reduced Motion behavior.
- Narrow-window Settings navigation uses the localized settings title.
- Home task-intent Hover and Focus no longer translate vertically.
- Removed the unused companion poster component, CSS, and four translations.
- Raised the Home workspace Portal to the established workspace-menu layer so
  conversation-side floating content cannot cover its directory-picker action.
- A follow-up Windows screenshot showed the remaining failure belonged to the
  sidebar workspace picker, not the Home composer picker. The sidebar picker is
  now portaled from its transformed, scrolling sidebar ancestry into
  `document.body`, preserving its fixed geometry while escaping that stacking
  context.
- Updated tests to validate current behavior contracts instead of source-string
  layouts that were moved into shared modules.

## Automated evidence

- Focused Home, sidebar, Settings, market, theme, and visual-system suite:
  61 passed, 0 failed.
- Full UI suite: 2,551 passed and 3 failed. The failures are existing
  conversation transcript, auto-scroll, and Skill-history baselines outside
  the Home and Settings scope.
- i18n types were regenerated after removing the retired translation keys.
- `check:i18n`, `check:theme`, `check:icons`, CodeMirror runtime, process-runtime
  boundary, browser-platform boundary, help registry, and `git diff --check`
  passed. All six built-in theme files passed the static theme contract.
- `typecheck` remains blocked by the existing `reasoning_effort` DTO mismatch
  and missing `@mediapipe/tasks-vision` video dependency. No Phase 2D file is
  present in the error set.
- `build:ui` remains blocked by the same missing MediaPipe dependency after
  Vite transformed 10,419 modules.
- The aggregate `check` command stops at the existing TypeScript baseline.
  Running the remaining gates individually found four existing retired
  vocabulary references in README, image analysis, and Xiaozhi documentation.

## Manual evidence boundary

Automated checks do not establish visual acceptance. Classic Light, the six
built-in Light/Dark pairs, 1440x900, 1280x800, 150% scale, narrow Settings row
boundaries, Preset Drawer boundaries, keyboard focus return, and Reduced Motion
remain Windows/Web manual acceptance unless separately recorded below.
