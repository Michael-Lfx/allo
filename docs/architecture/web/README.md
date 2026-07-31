# Web Search & Fetch Architecture

This is the entry point for Flowy's `web_search` / `web_extract` architecture.
Read this first when working on Search providers, MCP Fetch, local extraction,
host composition, privacy, budgets, or lifecycle.

## Reading order

1. [managed-web-search-fetch-evolution.md](managed-web-search-fetch-evolution.md)
   - timeline, current implementation summary, decisions, and pitfalls
2. [managed-web-search.md](managed-web-search.md)
   - current Search architecture and routing policy
3. [managed-web-fetch-policy.md](managed-web-fetch-policy.md)
   - current Local-first Managed Extract policy
4. [../../reference/web-search-provider-matrix.md](../../reference/web-search-provider-matrix.md)
   - provider admission evidence

## Historical records

Historical research, planning, and decision evidence lives outside this folder:

- `docs/superpowers/plans/2026-07-30-managed-web-search-you-rollout.md`
- `docs/superpowers/specs/2026-07-29-free-search-services-research.md`
- `docs/continuity/decisions/2026-07-31-parallel-web-fetch-admission.md`

## Model surface

The model only sees:

```text
web_search(query, count)
web_extract(urls)
```

All providers, MCP tools, fallbacks, `final_url`, session IDs, and diagnostics
stay private behind the agent tools.
