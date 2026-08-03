# Managed fetch evaluation

This directory contains the versioned public preflight corpus and its schema.
It is used only with the opt-in `fetch-eval` feature; normal application
builds do not load or execute it.

## v6 public qualification and Warm Pilot (2026-08-02)

The v6 corpus is frozen as `2026-08-02-public-admission-v6`. Two Local-only
attempts qualified 16 new official PDF cases as production-remote-eligible;
four fixed-order RFC candidates returned HTTP 404 twice and remain disabled.
The four previously qualified public PDFs plus the first new FIPS candidate
form the five-case `pilot-v6` PDF set. SP 800-53 Rev. 5 and ECMA-262 remain
diagnostic-only.

The fixed-order JS candidate pool was also checked twice per case (15
candidates, no MCP calls). Five existing shell cases remain the only stable
admission cases. RunKit was the only additional remote-eligible failure, but
it is not promoted because the existing response-shape probe did not provide
safe requested-URL identity. Challenge, 403/404, and Local-success candidates
remain excluded. Consequently `admission-v6` has 20 PDFs but only 5 JS cases;
the formal 15-case-per-category Admission gate is intentionally not started.

The 30-attempt Warm Pilot completed from clean SHA `e90045a0` using 30 actual
Fetch calls (15 per category), with no Search calls or recovery calls. Safety
five-field counts were all zero. PDF: 5/5 eligible and effective incremental
successes, Q2+ 5/5, Warm P50 1322 ms and P95 2382 ms. JS: 5/5 eligible and
effective incremental successes, Q2+ 5/5, Warm P50 503 ms and P95 1294 ms.
Both categories remain `insufficient_evidence` because each has only five
independent cases. Evidence is retained under the ignored
`fetch-evaluation-raw/v6-pilot/` directory and summarized by the ADR.

The resumable command is available for a future qualified campaign:

```bash
cargo run -p flowy-web --features fetch-eval --example fetch_eval -- campaign \
  --manifest crates/agent/flowy-web/evaluation/corpus.public.json \
  --tag admission-v6 --batch-size 5 --pacing-ms 3000 \
  --max-calls-per-batch 25 --daily-cap 60 --campaign-cap 200 \
  --output-dir fetch-evaluation-raw/admission-v6-<campaign-id>
```

It refuses dirty or changed provenance, persists each batch atomically, and
does not run while a category is below the 15-case Admission minimum.

## v5 Local-only qualification

The v5 corpus promotion was based on two Local-only attempts per candidate;
no MCP call was made during classification. The fixed-order result was:

| candidate | Local classifications | decision |
| --- | --- | --- |
| NIST SP 800-53 Rev. 5 | `Pdf`, `Pdf` | promoted as the fifth PDF |
| AST Explorer | Local success, Local success | excluded from fallback |
| ESLint Code Explorer | `JavascriptShell`, `JavascriptShell` | first qualifying JS; promoted |
| Babel REPL | Local success, Local success | excluded from fallback |
| MDN Playground | Local success, Local success | excluded from fallback |
| JSitor | `JavascriptShell`, `JavascriptShell` | retained as a later candidate |
| TryJS | Local success, Local success | excluded from fallback |

The NIST markers are present in the public publication text, and the ESLint
page exposes “ESLint Code Explorer” and “Abstract Syntax Tree” in its public
application shell. Local-success controls do not consume remote quota. The
resulting `preflight` tag therefore contains exactly five PDFs and five JS
cases; the v5 preflight itself remains capped at ten MCP calls and cannot
produce an enablement decision.

The committed `preflight` tag is allowed to select five public PDFs and five
public JavaScript shells. The selection is first classified with Local-only
runs;
Local-success controls and challenge pages remain in the corpus for diagnosis
but are not sent to MCP. Add only public, non-sensitive URLs and markers after
manually verifying them. Keep the private pypdf case in a local manifest such
as `fetch-evaluation.local.json`:

```json
{
  "schema_version": 1,
  "corpus_version": "2026-08-01-local",
  "cases": [
    {
      "id": "real-pypdf-private",
      "category": "real_pdf_private",
      "url_env": "ALLO_FETCH_CASE_REAL_PDF",
      "expected_markers": ["private marker"],
      "minimum_content_chars": 500,
      "minimum_marker_hits": 1,
      "verified_at": "2026-08-01",
      "stale_after_days": 30,
      "enabled": true,
      "notes": "URL is supplied only through the local environment."
    }
  ]
}
```

Run with:

```bash
cargo run -p flowy-web --features fetch-eval --example fetch_eval -- demo --output fetch-evaluation-demo.json
cargo run -p flowy-web --features fetch-eval --example fetch_eval -- run --mode compare --peer-mode cold --manifest crates/agent/flowy-web/evaluation/corpus.public.json --tag preflight --repeat 1 --pacing-ms 3000 --max-calls 10 --daily-cap 60 --output fetch-evaluation-run.jsonl
cargo run -p flowy-web --features fetch-eval --example fetch_eval -- admit --category public_pdf_text --manifest crates/agent/flowy-web/evaluation/corpus.public.json --tag preflight --pacing-ms 3000 --max-calls 15 --daily-cap 60 --output fetch-evaluation-admit.jsonl
cargo run -p flowy-web --features fetch-eval --example fetch_eval -- summarize --input fetch-evaluation-run.jsonl --output fetch-evaluation-summary.json
```

`demo` is fully offline and exercises Local, MCP, Compare, E2E, cold/warm
lifecycle, sensitive URL blocking, source mismatch detection and first-429
stop behavior. The JSONL result is deliberately sanitized. It contains case
IDs, categories, quality and timing metrics, but never complete URLs, query
strings, fragments, page text, cookies, authorization values, or raw MCP
payloads. The existing `parallel_web_fetch_probe` remains the explicit
developer tool for inspecting provider response shapes; this Runner does not
have a raw-output switch.

Cases whose `verified_at` date is beyond `stale_after_days` are skipped and do
not enter formal statistics. The Runner hard-caps a run at 25 calls and the
local, file-locked quota ledger at 60 calls per UTC day. A quota reservation is
made immediately before each actual remote tool call. The first 429 writes a
bounded `retry_after_ms` and cooldown to the status file, flushes the result,
and stops without sleeping or retrying. `search-warmed` performs one Search
initialization on the shared peer before Fetch attempts and consumes one quota
call. `warm` performs transport/tool initialization before timing; it does not
send a pre-warm Fetch request.

Only Compare and E2E records with a real Local failure enter admission
statistics. An Admission profile emits exactly one cold Compare and two warm
E2E attempts; a case needs at least two of three real Local failures, at least
one warm Remote attempt, two of three Remote effective successes, and one warm
effective success. Summaries are schema 2 evidence records: they require one
matching schema-3 status and schema-2 Safety Report per run, complete and
non-running terminal state, matching provenance, exact call counters, and a
recomputed five-field zero Safety gate. Legacy, dirty, missing, running,
truncated, or inconsistent evidence is readable for diagnosis but is always
marked incomplete and cannot produce `candidate_for_enablement`.

The `admit` command has no dirty-worktree or Git SHA override. Production runs
read the real HEAD; only the internal test factory can inject metadata. Status
and Safety files are written through a flushed, synced temporary file and
atomic replacement, while each JSONL attempt is flushed immediately. The
feature-gated runner sends only the production Fetch argument object
`{"urls": [...], "full_content": false}` through the same MCP call gate used
by Fake/Wiremock tests.
