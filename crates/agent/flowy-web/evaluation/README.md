# Managed fetch evaluation

This directory contains the versioned public preflight corpus and its schema.
It is used only with the opt-in `fetch-eval` feature; normal application
builds do not load or execute it.

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
effective success. Summaries reject mixed corpus, scoring, profile or safety
provenance and expose Wilson intervals, percentile timings, threshold
sensitivity, and an automatically written zero-valued Safety Report gate.
Missing or incomplete evidence cannot produce `candidate_for_enablement`.
