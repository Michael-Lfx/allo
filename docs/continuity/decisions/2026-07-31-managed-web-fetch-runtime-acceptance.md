# Managed Web Fetch Runtime Acceptance

Date: 2026-07-31

Status: PARTIAL / BLOCKED

## Environment

- Formal branch: `feat/managed-web-fetch`
- Formal branch SHA: `1ca065e6`
- Acceptance worktree branch: `verify/managed-web-fetch-runtime`
- Acceptance worktree commit: `cf1225f8`
- OS: Microsoft Windows NT 10.0.26200.0
- Rust/Cargo: `1.95.0`
- Bun: `1.3.14`
- Parallel endpoint reachable: yes

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

Full desktop startup in the acceptance worktree could not be completed:

- `bun run dev` fails because `react-virtuoso` is imported by the UI but not
  declared in `ui/package.json`.
- Cargo dev build also fails on `sccache.exe` until the wrapper is explicitly
  disabled; the default dev command does not set it.

Because the full desktop conversation flow was not started, these remain
unverified:

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
