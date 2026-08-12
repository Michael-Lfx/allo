# Settings UI/UX Phase 2C.1: Market Hub Hardening

## Purpose

Reconcile the rebased interaction-style branch with the market behavior that
prevents duplicate additions, then fix the shared card grid used by Skills,
Preset Packages, MCP, and Plugins.

## Scope

- Add shared `MarketCardGrid` and `MarketCardShell` layout contracts.
- Remove the market-card `h-full` sizing conflict and verify real browser geometry.
- Merge authoritative installed/imported state with transient card and drawer
  action state.
- Keep cards as summaries with an explicit details action and a shared detail
  drawer.
- Preserve existing routes, cache keys, DTOs, install flows, MCP review, and
  Preset persistence.

## Non-goals

No backend changes, market API changes, new dependencies, new market routes,
card-wide click behavior, fixed global card heights, or decorative motion.

## Contract

`MarketCardGrid` uses responsive CSS Grid with `align-items: stretch` and a
12px gap. `MarketCardShell` is `box-border`, natural-height, and self-stretching;
market cards must not use `h-full`. `MarketPrimaryActionConfig` may resolve a
consumer-owned checking, ready, completed, or error state. The shared action
hook adds pending state and exposes the same result to cards and the detail
drawer.

## Acceptance

- No overlap between rows at 1440px, 1280px, 150% scale, or narrow widths.
- Same-row card heights differ by no more than 0.5px.
- Completed Skills, Presets, and MCP entries cannot be submitted again.
- Card and drawer action labels, loading, disabled, and completed states match.
- Details are readable without opening an external source.
- Classic light and all built-in theme pairs preserve contrast and semantic
  state meaning.

See the Phase 2 history and the companion audit for evidence boundaries.

Follow-up: [Phase 2D visual contract closure](2026-08-12-home-settings-visual-contract-phase-2d.md).
