# Flowy GPUI Design Rules

Rules for desktop UI in `flowy18_wip`. Prefer these over inventing new layout or chrome patterns. Broader architecture lives in `docs/ui-system.zh.md`.

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

## Interaction surfaces

- Prefer `flowy_ui::interactive` / `Button` / `TextField` over bare `div().on_click()` for operable controls.
- Keep element ids stable across frames; do not key focus identity on translated labels.

## Consistency checklist for new UI

1. Popup uses `modal_layer` + `dialog` + `modal_radius`.
2. Opaque regions that touch rounded edges set their own matching `rounded_*`.
3. Cards with top media round the media, not only the card.
4. Page content uses padding + `max_w` so Create / grid / actions keep a right gutter.
5. Reuse `ResolvedTheme` tokens (`card`, `soft`, `line`, `fill`, `muted`) before adding new colors.
