# Managed Web Fetch Runtime Acceptance

Date: 2026-07-31

Status: PARTIAL

## Environment

- Formal branch: `feat/managed-web-fetch`
- Formal branch SHA: `1ca065e6`
- Acceptance worktree branch: `verify/managed-web-fetch-runtime`
- Acceptance worktree commit: `cf1225f8`
- OS: Microsoft Windows NT 10.0.26200.0
- Rust/Cargo: `1.95.0`
- Bun: `1.3.14`
- Parallel endpoint reachable: yes

## Dependency Fix

`react-virtuoso` was missing from `ui/package.json`. It was added in
`f2357a9a fix(ui): declare missing react-virtuoso dependency`, installed in
the acceptance worktree, and `bun run dev` now starts successfully.

## Executed

The real Parallel `web_fetch` probe was run with a stateful MCP session:

- static HTML
- PDF
- JavaScript shell / SPA
- Chinese article
- short page
- long page
- two successes
- one success + one failure
- two successes + one failure
- HTTP 404
- HTTP 403
- duplicate URLs
- redirect

All probe cases returned HTTP 200 with structured content and no raw page text
was printed. Discovery observed protocol `2025-11-25`, stateful session,
`initialize_ms=280..333`, `tools_list_ms=268..306`, tool count `2`, and
`web_fetch` present.

## Blocked

Desktop startup passed:

- Vite dev server ready on `http://127.0.0.1:5173/`
- `Flowy.exe` started
- Desktop backend serving on loopback

Real desktop session evidence:

- `managed_search` succeeded through `provider=parallel`
- `web_extract` completed with `requested_count=3`
- `local_success_count=3`, `final_success_count=3`
- `remote_eligible_count=0`, `remote_attempted=false`
- `context_truncated_count=3`

The desktop session has not yet exercised a real Parallel remote fallback, so
these remain unverified:

- Local-only call count with `managed_extract=true`
- Model output shape in a real conversation
- Remote forbidden zero-outbound assertions through a real proxy
- MCP session expiry recovery in the app
- cancel / shutdown matrix
- performance sample matrix

## Recommendation

Do not enable `managed_extract=true` on the formal branch yet. The real
Parallel fetch path is reachable and the core remote cases passed, but full
runtime acceptance is not complete.

## Pending Acceptance Work

- Re-run the pypdf PDF in the acceptance worktree and confirm
  `remote_attempted=true`.
- Before retrying, provisionally adjust `RemoteBudgetPolicy` in the acceptance
  worktree only:
  - warm fetch budget: 3 seconds
  - cold fetch budget: 5 seconds
  - safety margin: 1 second
- Collect 5 cold PDF samples and 10 warm PDF samples before finalizing the
  production budget constants.
- Evaluate whether the desktop host should pre-warm the shared Parallel peer at
  startup so common warm-path calls are not treated as cold.
- Complete privacy zero-outbound verification, MCP session recovery, cancel,
  shutdown, and performance samples.
- Keep `managed_extract=false` on the formal branch until acceptance passes.

## Merge Notes

Merging `feat/managed-web-fetch` to main is acceptable only while
`managed_extract=false`. The formal branch does not enable remote fetch in
production. Do not merge `verify/managed-web-fetch-runtime` because it contains
the temporary `managed_extract=true` acceptance commit.
