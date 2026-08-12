# Home and Settings UI/UX Phase 2D: Visual Contract Closure

Follow-up interaction fixes: [Workspace picker and Preset editor rebase
follow-up](2026-08-12-uiux-rebase-popover-preset-fixes.md).

## Purpose

Close the remaining implementation and verification drift after the Home and
Settings redesign without changing their information architecture or business
flows. Ink Studio remains the visual foundation and Quiet Kinetic remains the
behavioral layer.

## Changes

- Use the shared Warning theme role in the Preset editor.
- Animate only the Settings tab indicator transform, never its width.
- Localize the narrow-window Settings navigation label.
- Keep Home task-intent chips spatially stable on Hover and Focus.
- Remove the unmounted companion-poster component, its decorative theme CSS,
  and its private translations.
- Update structure tests to assert the current shared navigation, send routing,
  semantic selection palette, and scroll ownership contracts.

## Compatibility

No backend API, route, Composer prop, market DTO, Preset DTO, persistence,
extension protocol, or theme interface changes. The hidden Mini App Home entry
remains hidden while its explicit deep-link compatibility remains intact.

## Acceptance

- Focused Home and Settings tests pass with no stale assertions.
- Theme, i18n, and icon contract checks pass.
- Home Composer and task intents do not move on Hover or Focus.
- Settings Warning, Selected, and Focus states remain legible across theme pairs.
- Manual browser and Windows evidence remains distinct from automated checks.
