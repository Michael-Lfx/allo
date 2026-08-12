---
name: Flowy
description: A restrained, precise, and alive workspace for agent-driven work.
colors:
  ink-accent-light: "#0f766e"
  ink-accent-dark: "#2dd4bf"
  canvas-light: "#f8fafc"
  panel-light: "#ffffff"
  text-light: "#0f172a"
  canvas-dark: "#0b1220"
  panel-dark: "#111827"
  text-dark: "#f8fafc"
  success-light: "#15803d"
  warning-light: "#b45309"
  danger-light: "#b91c1c"
typography:
  display:
    fontFamily: "Segoe UI Variable, Segoe UI, PingFang SC, Noto Sans SC, system-ui, sans-serif"
    fontSize: "28px"
    fontWeight: 600
    lineHeight: 1.25
  title:
    fontFamily: "Segoe UI Variable, Segoe UI, PingFang SC, Noto Sans SC, system-ui, sans-serif"
    fontSize: "20px"
    fontWeight: 600
    lineHeight: 1.25
  body:
    fontFamily: "Segoe UI Variable, Segoe UI, PingFang SC, Noto Sans SC, system-ui, sans-serif"
    fontSize: "14px"
    fontWeight: 400
    lineHeight: 1.5
  label:
    fontFamily: "Segoe UI Variable, Segoe UI, PingFang SC, Noto Sans SC, system-ui, sans-serif"
    fontSize: "12px"
    fontWeight: 500
    lineHeight: 1.4
rounded:
  control: "8px"
  panel: "12px"
  focal: "16px"
  pill: "999px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "12px"
  lg: "16px"
  xl: "24px"
  xxl: "32px"
components:
  button-primary:
    backgroundColor: "{colors.ink-accent-light}"
    textColor: "{colors.panel-light}"
    rounded: "{rounded.control}"
    height: "36px"
  surface-panel:
    backgroundColor: "{colors.panel-light}"
    textColor: "{colors.text-light}"
    rounded: "{rounded.panel}"
    padding: "{spacing.lg}"
  composer:
    backgroundColor: "{colors.panel-light}"
    textColor: "{colors.text-light}"
    rounded: "{rounded.focal}"
    padding: "{spacing.lg}"
---

# Design System: Flowy

## Overview

**Creative North Star: "Quiet Kinetic Workspace"**

Ink Studio remains Flowy's static visual foundation: disciplined density,
ink-like structure, and a restrained accent. Quiet Kinetic adds the behavioral
layer. The workspace is calm at rest and becomes visibly responsive only when
the user focuses, selects, sends, reveals, or recovers.

The physical scene is a user working for an extended session on a desktop,
alternating between reading dense results and issuing short commands. The UI
must remain comfortable in bright and dim environments while making agent
activity understandable at a glance.

Key characteristics: task-first composition, tinted neutral surfaces, clear
spatial layers, compact familiar controls, and short directional motion.

## Colors

Flowy uses a restrained strategy. Themes own hue and atmosphere; semantic roles
own meaning. Canvas, panel, raised, and interactive surfaces must remain
distinguishable in every built-in theme. Accent color is reserved for primary
actions, current selection, keyboard focus, and live state.

Built-in themes define paired light and dark values. Primary text on the
canvas, panel, and raised surfaces must retain at least 4.5:1 contrast. Status
must combine color with text, icon, or shape. User and extension CSS remain an
advanced, unrestricted override and are not silently repaired.

Background images may decorate the canvas, but critical text and controls sit
on stable readable surfaces. Popovers, menus, modals, and the composer may use
translucency only when their resulting readability matches an opaque surface.

## Typography

Use the system-oriented sans stack throughout product UI. Hierarchy comes from
size, weight, and spacing rather than decorative typefaces. Body copy stays at
14px with a 1.5 line height; prose is capped near 70 characters per line.
Labels and metadata remain compact but must not be reduced until contrast or
scanability suffers.

## Elevation

Four semantic surfaces define depth:

- Canvas is the application backdrop.
- Panel holds persistent navigation or grouped settings.
- Raised holds focused work such as the composer and temporary overlays.
- Interactive identifies an item that can be selected, pressed, or dragged.

Prefer tonal separation and hairline borders. Shadows are structural and
ambient, not decorative. Avoid nested cards. Motion reinforces elevation with
opacity and transform only: 120ms for feedback, 180ms for state changes, and
240ms for spatial transitions. Use exponential ease-out curves; never bounce.

## Components

Every interactive control implements default, hover, focus, active, disabled,
loading, and error where applicable. Controls use 8px corners, structural
panels 12px, and the task composer 16px. Pills are limited to short status and
filter labels.

The sidebar uses one spatial selection indicator shared across entries.
Hover and press feedback must not reflow labels or action slots. The home
composer is the dominant interaction surface; model, permission, and workspace
configuration remain available but visually secondary until invoked.

Settings reuse this principle inside their own navigation container only. The
settings shell is Page Header → Section → List → Row: a Panel-backed list has
one subtle boundary and row separators, never a stack of nested cards. Rows
keep text left and a 220–280px control slot right on desktop, then stack below
768px. A disabled state keeps its explanatory copy in place, and an error or
restart-required state is concise, local, and readable without color alone.
Desktop and narrow-window settings navigation share one four-group model
(Application, Intelligence & Content, Capabilities & Extensions, Account &
Other). Extension entries inherit their host group through compatible anchors;
missing hosts fall back to Capabilities. The settings selection indicator stays
inside its own scrolling container, so it never travels through the app footer
or another semantic surface.

Settings detail pages use the same contract rather than local page skins.
`form` content is capped at 960px; list- and market-oriented `hub` content is
capped at 1180px. Page changes use a 120ms opacity-only transition. Explicit
save flows reveal a compact sticky action bar only after a value differs from
its last confirmed value; a failed save keeps that draft and reports locally.
Auto-saved controls remain quiet on success. Capability hubs use textual tabs
with a 2px moving underline, keyboard focus, and the same semantic selected
color as the navigation rail—never a filled-pill tab treatment.

Settings controls use an explicit slot contract: `compact` for switches and
short state, `field` for selects and text inputs, `actions` for button groups,
and `compound` for related controls such as a slider plus numeric input. Rows
respond to their own container, not only the viewport. Wide action groups move
as a whole before any icon-label pair can break, and button labels never wrap
inside their control. Long explanatory copy is capped near 70 characters per
line.

Capability hubs use a minimum 270px responsive card column. Cards in a row
share their height for scanability, while the grid remains content-driven
between rows. A card clamps its title and description, keeps its primary action
intact, and shows at most two high-value metadata tags before a localized
`+N`; long technical labels truncate with their full text available through the
in-app detail view. Source selection belongs to the toolbar, not every card.
Refresh and search are icon actions, while three or more import variants are
grouped under one menu. Hub canvas, filtering panel, and object cards must
remain distinct layers, without a decorative outer card wrapping them all.

Marketplace cards are passive summaries: no card-level hover, selected border,
or decorative rank color. Controls alone carry Hover, Press, Focus and Loading
feedback. “View details” opens a local detail drawer containing the complete
description, metadata, tags and install command; “Open source” remains a
secondary external action. The toolbar has breathing room after page tabs and
does not repeat a large heading already supplied by the page shell.

The shared market layer is model-first: `MarketItemViewModel` owns localized
statistics, bounded summary tags, complete detail metadata, API requirements,
and source links; `useMarketCatalog` owns cache and filter state; and
`useMarketActionState` owns the single active primary action shared by cards and
the detail drawer. A catalog status is explicit (`loading`, `cached-refresh`,
`ready`, `empty`, `no-match`, or `partial-error`) so a failed refresh never
silently replaces usable cached results. The toolbar measures its own control
slot with `ResizeObserver` before choosing a segmented source picker or a
select, rather than guessing from viewport width.

The preset editor stays a single right-side drawer: desktop width is capped at
760px when at least 840px is available, its header and footer stay fixed, and
its body is the only scroll owner. It uses sections and hairlines rather than a
large nested gray container. Close, Escape and mask dismissal ask before
discarding a dirty draft. Agent-skill import uses an embedded presentation
inside that same drawer, preserving the parent draft and avoiding stacked
drawers. While importing, the editor footer is hidden so there is one action
surface. Closing, saving, deleting, and cancelling restore focus to the
original trigger; failed validation reveals, scrolls to, and focuses the first
invalid field.

`PresetDraft` is the editor boundary. Its normalized signature sorts only
set-like fields, while target and model order remains semantic. The editor
exposes `draft`, `updateField`, `dirty`, `valid`, `save`, `discard`, `saving`,
and `fieldErrors`; the visible drawer keeps the legacy field controls wired to
that contract during migration. Independent `AgentSkillImportDrawer` and
embedded `AgentSkillImportContent` share business logic without sharing two
layout modes in one component.

Reduced motion removes travel and reveal animation while preserving immediate
state changes. Animation never delays navigation, typing, or sending.

### Phase 2D visual contract closure

Home task-intent chips keep their geometry stable on Hover and Focus; only
their foreground, background, and boundary change. Press may use the shared
subtle scale feedback. The composer remains completely stationary across
Hover, Focus, and Active states.

Settings tab indicators update their measured width immediately and animate
only their transform. Warning presentation is owned by the theme warning token,
never a page-local RGB value. Narrow-window navigation exposes its accessible
name through the same localized settings title as the desktop shell.

Temporary workspace menus use the shared `WorkspacePickerPopover`, rendered
into `document.body` above persistent conversation-side floating content. The
picker owns fixed trigger-relative placement, viewport bounds, resize and
scroll updates, dismissal, and focus return. Every action, including the final
directory-picker action, remains pointer reachable without relying on a parent
stacking context.

Unmounted visual experiments must not leave production components, theme-
specific CSS, or translation keys behind. A retired experiment is removed as
one unit once its runtime call sites reach zero.

## Do's and Don'ts

Do:

- Keep action and result spatially connected.
- Use semantic tokens rather than theme-specific hard-coded colors.
- Verify all built-in presets in both light and dark modes.
- Use whitespace to group settings and information.
- Make focus, selection, progress, and recovery independently recognizable.

Don't:

- Wrap every message, setting, or list row in an equal card.
- Use gradient text, colored side stripes, or decorative looping glow.
- Depend on blur or a background image for text contrast.
- Animate layout properties or orchestrate page-load sequences.
- Change input, paste, send, or persistence behavior during a visual pass.

### Phase 2C.1 market hardening

The market implementation uses `MarketCardGrid` and `MarketCardShell` as the
single layout contract for Skills, Presets, MCP, and Plugins. Grid rows stretch
only within their own row, cards are `border-box` with natural height, and
market cards must not use `h-full`, fixed global heights, or decorative hover
frames. Card summaries expose a keyboard-accessible details action; complete
metadata and install commands live in the shared detail drawer.

Primary actions merge authoritative completion state with transient pending
state. Consumers resolve `checking`, `ready`, `completed`, or `error`; the
shared action hook adds `pending` and keeps the card and drawer synchronized.
Completed items remain disabled until the consumer's reliable installed or
imported query confirms otherwise. Geometry acceptance requires no row overlap,
same-row height alignment within 0.5px, and a minimum 12px gap between rows.
