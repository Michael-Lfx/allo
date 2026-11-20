# Flowy GPUI Design Rules

Rules for desktop UI in `flowy18_wip`. Prefer these over inventing new layout or chrome patterns.

## Modal shells

- Use `ResolvedTheme::modal_radius()` for every popup / dialog shell. Do not invent a one-off radius.
- Present modals with `flowy_ui::modal_layer` (scrim + blur + outside dismiss) and `flowy_ui::dialog` for the panel.
- Large modals size with `theme.modal_size(viewport)`.
- Business open state belongs in `ActiveDialog`; OverlayHost only owns presentation / focus.

## GPUI corner clipping (critical)

GPUI `overflow_hidden` clips to a **rectangle only**. It does **not** clip children to the parent's corner radius.

Symptoms:

- Parent has `rounded(...)` + `overflow_hidden`, but opaque child backgrounds still paint square corners over the curve.
- Top header / side nav / cover image “cuts” the rounded corner.

Required pattern: any opaque child that sits against a rounded edge must carry the matching corner radius itself.

| Region | Child fix |
| --- | --- |
| Top strip / header | `.rounded_t(theme.modal_radius())` |
| Bottom strip / footer | `.rounded_b(theme.modal_radius())` |
| Left pane | `.rounded_l(theme.modal_radius())` |
| Right pane | `.rounded_r(theme.modal_radius())` |
| Cover / media flush to card top | `.rounded_t_xl()` (or same radius as the card) |

Also:

- Keep scrollable lists out of the corner band when needed (`pb(theme.modal_radius())` on the pane), same as settings nav/main.
- Do not rely on parent `overflow_hidden` alone for rounded chrome.

## Cards with media

- Card shell: `.rounded_xl().border_1().overflow_hidden()`.
- Top media / tint block: also `.rounded_t_xl()` so fill follows the top curve (same clipping rule as modals).

## Content width / margins

- Full-bleed More pages (Skills, Personal): scroll host with horizontal padding, then an inner column with `w_full().max_w(...).min_w_0()`.
- Do not stretch header actions / grids to the window edge on wide screens; right margin comes from the capped column, not only from padding.
- Prefer `min_w_0` on flex children that can overflow horizontally.

## Motion tokens

Durations and curves live in `crates/flowy-studio/src/motion.rs`. Never write
`Duration::from_millis` inline for UI motion. The values and curves themselves
are specified in the repository `DESIGN.md`; this file only says where they live.

Entrance needs travel, not just opacity: a fade alone is invisible on the dark
canvas. Pair it with `motion::ENTER_OFFSET`.

## Interaction surfaces

- Prefer `flowy_ui::interactive` / `Button` / `TextField` over bare `div().on_click()` for operable controls.
- Keep element ids stable across frames; do not key focus identity on translated labels.

## Button hierarchy

Choose a button's variant from its role in the current task, not its position or label length. Keep the existing neutral palette and use `ControlColors` for the complete foreground/background/border pairing.

| Priority | Variant | Use |
| --- | --- | --- |
| Primary action | `Primary` | The action that completes or advances the current task: send, connect, create, confirm, or run. At most one per visible action group. |
| Alternative action | `Secondary` | Cancel, go back, resend, or another available path. Also the safe default for `Button::new`. |
| Supporting action | `Ghost` | Copy, share, reveal, expand, toolbar tools, and close. Icon-only actions use `IconButton` with a localized accessible name. |
| Destructive action | `Danger` | An action that deletes or irreversibly discards user data. Stopping generation or closing a dialog is not destructive by itself. |

- Mark every primary action explicitly with `.variant(ControlVariant::Primary)`; do not make all buttons in a footer equally prominent.
- Use `Button` / `IconButton` for ordinary actions. Reserve `interactive()` for navigation, selection, custom cards, and other surfaces whose semantics require a separate component recipe.
- Keep the primary variant while disabled or loading so its role stays recognizable. Disabled actions leave the Tab path; loading actions retain focus and reject repeat activation.
- In a footer, place the alternative before the primary action in the existing reading order. Independent cards may each have a primary action, but a single toolbar or footer must not contain competing primary buttons.
- Use `Small` for compact tools, `Medium` for standard actions, and `Large` for prominent full-width submissions. Text and icons inherit the variant's foreground; do not override it with `ink`, `on_ink`, or authored colors.
- Keep toggle, selected, and navigation states separate from action priority. A selected filter or active navigation item is not a primary button.

## Consistency checklist for new UI

1. Popup uses `modal_layer` + `dialog` + `modal_radius`.
2. Opaque regions that touch rounded edges set their own matching `rounded_*`.
3. Cards with top media round the media, not only the card.
4. Page content uses padding + `max_w` so Create / grid / actions keep a right gutter.
5. Reuse `ResolvedTheme` tokens (`card`, `soft`, `line`, `fill`, `muted`) before adding new colors.
