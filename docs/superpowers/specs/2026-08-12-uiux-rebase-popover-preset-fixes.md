# UI/UX Rebase Follow-up: Workspace Picker and Preset Editor

## Purpose

After rebasing the interaction-style work onto `origin/main`, resolve the two
reviewed interaction seams without changing workspace persistence, Preset DTOs,
or install workflows.

## Scope

- Replace per-page workspace menus with one body-level picker layer.
- Keep the directory-picker action above homepage, knowledge-base, and
  scheduled-task content.
- Treat Preset skill import as an editor step with one visible action footer.
- Restore focus after Drawer completion or dismissal, and focus the first
  invalid field after a failed save.

## Contract

`WorkspacePickerPopover` owns portal placement, viewport avoidance, scroll and
resize recalculation, Escape and outside-dismissal, and trigger focus return.
Callers own only trigger state and the existing workspace callbacks. The Preset
editor has `editing` and `importingSkills` steps. The embedded import step owns
its actions; the parent Drawer footer is absent until the editor resumes.

## Non-goals

No new workspace persistence, directory-picker IPC, Preset fields, market
protocols, routes, or phone-specific layout.

## Acceptance

- The picker is visible and clickable above all three relevant page contexts.
- Escape and outside dismissal return focus to the trigger.
- Importing skills never presents a second Preset save/cancel footer.
- Saving, cancelling, deleting, and validation failure have predictable focus
  destinations while preserving the draft where required.
