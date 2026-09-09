# Agent conversation evaluation

Live and offline evaluation for the **real** Nomi harness / runtime.

Eval scores a pinned model plus `AgentEngine` (Office profile, or `CodingHarness` for coding suites). It is not a model leaderboard. CLI/library remain the source of truth; the desktop `/eval` page is a developer-mode lab console.

Each live case is bound to **Session Observation**: one `conversation_id`, `session_kind=eval`, the same `ObservationRecorder` JSONL path as ordinary sessions. The conversation shell projects thinking / tool_call / text (plus token usage and billing turn id) so ChatLayout looks like a real user session.

## What counts as an agent eval

Marker-style Q&A and single-function unit floors are **not** harness KPIs. The live catalog only lists agent suites:

| Suite | Source | Why it is (or is not) an agent task |
| --- | --- | --- |
| `harness_smoke` | bundled [`corpus.harness_control.json`](./corpus.harness_control.json) | **Regression smoke.** Write/Edit + `write_root`. Alias: `harness_control`. Fail = runtime broken. |
| `office_core` | bundled [`corpus.office.json`](./corpus.office.json) | **Primary office-agent suite.** Structural oracles (CSV/JSON/keywords), Office profile, not CodingHarness. Alias: `office_tasks`. |
| `coding_local` | bundled [`corpus.coding_local.json`](./corpus.coding_local.json) | Local coding (debug+pytest, CSV→JSON, refactor). Alias: `agent_workflows`. |
| `browser_smoke` | bundled [`corpus.browser_smoke.json`](./corpus.browser_smoke.json) | Browser overlay + local HTML fixture. No public web. |
| `mcp_fixture` | bundled [`corpus.mcp_fixture.json`](./corpus.mcp_fixture.json) | Injected stdio fake CRM. No third-party SaaS. |
| `private_badcases` | `{data_dir}/diagnostics/agent-evals/private/promoted.json` | Promoted cloud badcases. Empty suite is listable but cannot start. |
| `aider_polyglot` | [Aider polyglot-benchmark](https://github.com/Aider-AI/polyglot-benchmark) Python / Exercism | Advanced coding contrast. Not an official Aider score. |
| `classeval` | [ClassEval](https://github.com/FudanSELab/ClassEval) | Advanced class-level Python. |
| `harbor_terminal_bench` | Harbor/Docker placeholder | `requires_sandbox`. Lab disables Run. No unofficial Terminal-Bench number. |

`session_dialogue` remains available only as the offline CLI demo corpus (`corpus.conversation.json`). It is **not** listed in the live lab catalog.

Downloads cache under `{data_dir}/diagnostics/agent-evals/datasets/` (default limit 8, max 20).
Pull tries GitHub raw first, then a jsDelivr CDN mirror when raw is unreachable.

### Not wired (and must not be faked)

These are the 2025–2026 coding-agent standards. They need Docker / extra tool surfaces this eval lab keeps off:

| Benchmark | Gap |
| --- | --- |
| **SWE-bench** Verified / Lite / Pro | Official score = clone repo at `base_commit`, apply agent patch, run fail-to-pass tests in Docker. No sandbox here → no official SWE-bench number. |
| **Terminal-Bench** | Harbor/Docker long-horizon shell tasks. |
| **GAIA** | Web search and browsing; eval isolation turns those off. |
| **τ-bench / τ²** | Mock customer-service APIs — would be a second tool surface. |
| **OSWorld / WebArena** | Computer-use / browser; disabled in eval isolation. |

## Isolation (live harness)

Live runs are assembled in `nomifun-ai-agent` (`LiveNomiHarness` + `EvalLab`), not in this crate, so `nomi-*` stays free of `nomifun-*`.

- Workspace: run starts with a business-named parent dir `{data_dir}/diagnostics/agent-evals/workspaces/评测-{suiteLabel}-{timestamp}-{shortRunId}/`, cases under `{case_id}/`
- `session.enabled = false` — no nomi session-file persistence; observation is wired explicitly
- Conversation shell: `{case_id} · {category}` via `EvalSessionBridge` (idempotent `eval:{run_id}:{case_id}`); trajectory projected with `with_flowy_billing_turn_id`
- Observation: `{data_dir}/diagnostics/observation/{conversation_id}/events.jsonl` (`session_kind=eval`)
- Not registered in `AgentRuntimeRegistry`
- `auto_approve = true`, `write_root` = eval workspace
- Isolation overlay is per suite: smoke / office / coding keep MCP and browser off; `browser_smoke` enables browser with a local HTML fixture; `mcp_fixture` injects a stdio fake CRM
- One in-flight run at a time (HTTP 409)
- Outcome scorers decide pass/fail; transcript scorers are constraints; rubric/advisory scorers do not flip the case
- History keeps 50 local summaries; UI can diff two runs and report `pass@1` / `pass^k`

## Commands

Requires the `agent-eval` feature (example binary only; the library always builds):

```bash
# Scripted offline pass (no LLM)
cargo run -p nomi-agent-eval --example agent_eval --features agent-eval -- \
  smoke

# Same corpus via OfflineDemoHarness (default harness)
cargo run -p nomi-agent-eval --example agent_eval --features agent-eval -- \
  run \
  --manifest crates/agent/nomi-agent-eval/evaluation/corpus.conversation.json \
  --tag local \
  --output /tmp/agent-eval-run.jsonl

# Cache an agent dataset (Aider Polyglot / ClassEval)
cargo run -p nomi-agent-eval --example agent_eval --features agent-eval -- \
  pull --suite aider_polyglot --cache-dir /tmp/agent-eval-cache --limit 8

# Aggregate JSONL → summary JSON
cargo run -p nomi-agent-eval --example agent_eval --features agent-eval -- \
  summarize --input /tmp/agent-eval-demo.jsonl --output /tmp/agent-eval-summary.json
```

Live production-harness runs go through the desktop API (`/api/debug/agent-evals/*`, developer mode) so they can use `AgentBootstrap` without this crate depending on the backend.

`budgets.max_tokens` is a cumulative **output-token** runaway cap (coding suites: 65536). It does not count prompt/context/input. The engine's per-request `max_tokens` (default 8192) is a separate generation limit.

## Evidence hygiene

Each JSONL line stores `case_id`, `category`, `success`, scorer results, tokens, timing, `conversation_id`, and trajectory/artifact **counts**.
The full per-case trajectory and workspace artifacts are written beside JSONL as
`{data_dir}/diagnostics/agent-evals/runs/{run_id}/traces/{case_id}.json` (relative paths only; prompts and tool IO redacted).
Prompt text is sanitized with `nomi-redact` so `sk-…` secrets never land on disk.
Workspace absolute paths are never serialized.

The `/eval` page polls `GET /api/debug/agent-evals/runs/{id}` during a live run (`current_trace` / `current_conversation_id`) and loads a finished case via `GET …/cases/{case_id}/trace` plus Session Observation via `GET …/cases/{case_id}/observation`.

The offline `OfflineDemoHarness` 100% pass rate is self-referential and is not a KPI for the live agent.

## Resume

`run` defaults to `--resume`: existing case_ids in the output JSONL are skipped.
Pass `--no-resume` to truncate and re-run. Live lab runs always start a new JSONL.

## Tests

```bash
cargo test -p nomi-agent-eval --all-targets
```
