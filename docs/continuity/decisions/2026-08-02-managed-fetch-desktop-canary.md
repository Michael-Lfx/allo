# Managed Fetch Desktop Evidence-Backed Canary

> Historical evidence: commit SHA fields in this record refer to the pre-rewrite
> checkpoint. The rebase-era merge decision is recorded separately.

> Historical record: the default-mode decision was made later. For the
> current state, see `2026-08-03-managed-fetch-current-state.md`.

Date: 2026-08-02
Branch: `feat/fetch-optimization`
Implementation commit: `487505121aad2d96bf3e8e73d3c3bdd59c2bcc77`
Run: `019fc0a1-3a69-7700-a6ac-c3e579d5e3bc`

## Scope

This is the Stage A canary for the evidence-backed Desktop mode. It used the
existing production `ParallelMcpClient`, the production call safety gate, the
quota gate, the local-first coordinator, and the exact Admission triple shape
for three selected public cases:

- one public PDF case;
- one JavaScript Shell case;
- one static HTML Local-success control.

Each case ran one cold Compare and two warm E2E attempts. The run was clean at
the recorded Git SHA. Raw JSONL, status, safety, and quota artifacts remain
local under `fetch-evaluation-raw/v7-canary/` and are ignored by Git.

## Result

| Category | Independent cases | Attempts | Remote attempts | Effective successes | Warm max |
|---|---:|---:|---:|---:|---:|
| Public PDF | 1 | 3 | 3 | 3 | 365 ms |
| JavaScript Shell | 1 | 3 | 3 | 3 | 545 ms |
| Static HTML control | 1 | 3 | 0 | 2 Local successes | 83 ms |

The PDF attempts were `Pdf` failures locally and returned Q3 remote content
with the required marker. The JavaScript attempts were `JavascriptShell`
failures locally and returned Q3 remote content with both required markers.
The static control never attempted remote extraction. No complete URL, body,
query, payload, cookie, header, or URL hash is stored in the evidence files.

## Safety and provenance

- actual calls: 6 Fetch, 0 Search, 6 total;
- recovery calls: 0;
- source mismatch: 0;
- dropped remote items: 0;
- sensitive egress: 0;
- retry-limit violations: 0;
- cancellation late results: 0 (no cancellation event was observed in this run);
- safety report: complete, `all_zero=true`;
- worktree: clean;
- status/safety/result Git SHA, corpus, scoring, and profile: consistent.

The run stopped normally with `stop_reason=completed`. It did not encounter a
429 or quota stop and did not use the remaining canary call allowance.

## Decision

`retain_experimental` — `insufficient_evidence`.

The implementation and the narrow canary passed the Stage A technical gates,
but one independent case per category cannot support a production enablement
decision or a formal public Admission result. Desktop therefore remains
Disabled by default. The explicit canary override is still available:

```text
NOMIFUN_MANAGED_FETCH_MODE=evidence-backed
```

The emergency Local-only override remains:

```text
NOMIFUN_MANAGED_FETCH_MODE=off
```

## Next approval boundary

The next action requiring the owner is one Desktop session with the explicit
`evidence-backed` override: verify a normal HTML page stays Local-only, then
verify one PDF and one JavaScript Shell use `web_extract` and report a remote
attempt with readable content. If that session passes, a separate approved
change may switch the unset Desktop default to `EvidenceBacked`; otherwise the
default stays Disabled. This canary is not the 15+ independent-case public
Admission and does not authorize production enablement by itself.
