# Managed Web Fetch Automated Gate Results

Date: 2026-07-31

## Environment

- Branch: `feat/managed-web-fetch`
- Branch SHA at audit start: `9c1d83e1`
- Baseline SHA: `c4ace616` (`origin/main`)
- OS: Microsoft Windows NT 10.0.26200.0
- Rust: `1.95.0`
- Cargo: `1.95.0`
- Bun: `1.3.14`
- Node: `22.22.2`

## Status

```text
Implementation complete
Managed Web targeted gates passed
Workspace compile passed
Desktop managed_extract=false
Runtime acceptance pending
```

## Passed Commands

- `cargo fmt --all -- --check`
- `cargo check --workspace`
- `cargo test -p flowy-web`
- `cargo test -p nomi-mcp`
- `cargo test -p nomifun-ai-agent --features managed-search --test managed_web_handle --test managed_search_handle`
- `cargo test -p nomifun-app --features managed-search services::tests::managed_web`
- `git diff --check`
- `bun run check` items through browser-platform-boundary

## Failed Commands

### `bun run check`

`check:agent-vocabulary` reports 8 retired active references:

- `crates/backend/nomifun-api-types/src/tv_show.rs`
- `ui/src/renderer/pages/videoGeneration/api.ts`
- `ui/src/renderer/services/i18n/i18n-keys.d.ts`
- `ui/src/renderer/services/i18n/locales/en-US/settings.json`
- `ui/src/renderer/services/i18n/locales/zh-CN/settings.json`

The Managed Web Fetch branch does not modify those files.

### `cargo test -p nomi-agent`

686 tests passed; 5 existing failures:

- `compact::auto::tests::below_threshold_does_not_trigger`
- `compact::auto::tests::threshold_pct_none_uses_default_logic`
- `compact::emergency::tests::below_limit_returns_false`
- `context::tests::prefix_stability_no_date_in_system_prompt`
- `engine::compact_tests::emergency_fires_when_at_limit`

These are compact/context baseline failures, not caused by Managed Web Fetch.

### `nomifun-ai-agent` full suite

Known `runtime_registry` hang. Managed Web Handle and Managed Search Handle
integration tests pass; the full suite is not run in the feature gate because
the hang predates this branch.

## Baseline Policy

These failures are recorded, not fixed in `feat/managed-web-fetch`. If the
repository requires all gates green before merge, they must be handled in a
separate baseline branch.

## Next Step

Runtime acceptance must be executed in a separate worktree with a temporary
`managed_extract=true` commit. The formal feature branch keeps
`managed_extract=false` until acceptance passes.
