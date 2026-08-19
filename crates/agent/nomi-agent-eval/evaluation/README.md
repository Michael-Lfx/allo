# Agent conversation evaluation

Live and offline evaluation for the **real** Nomi harness / runtime.

Eval scores a pinned model plus `AgentEngine` (Office profile, or `CodingHarness` for coding suites). It is not a model leaderboard. CLI/library remain the source of truth; the desktop `/eval` page is a developer-mode lab console.

## What counts as an agent eval

HumanEval / MBPP are **unit-coding floors**: one function, empty workspace, hidden asserts. They do not exercise Read/Edit/Bash loops and are **not** harness KPIs.

Faithful agent suites on this harness (no second agent loop, deterministic oracles):

| Suite | Source | Why it is (or is not) an agent task |
| --- | --- | --- |
| `office_tasks` | bundled [`corpus.office.json`](./corpus.office.json) | **Primary office-agent suite.** Memo, minutes, CSV budget, client email, in-place rewrite. Live harness uses the Office profile (Read/Write/Edit), not CodingHarness. |
| `aider_polyglot` | [Aider polyglot-benchmark](https://github.com/Aider-AI/polyglot-benchmark) Python / Exercism | **Primary coding-agent suite.** Stub + tests in the workspace; agent edits and may run pytest. Canonical `.meta/example.py` is stripped. Not an official Aider leaderboard score (that harness is Docker). Needs Python; pytest preferred, unittest fallback. |
| `classeval` | [ClassEval](https://github.com/FudanSELab/ClassEval) | Class-level Python (skeleton in `solution.py`, **hidden** unittests). Harder than HumanEval; still closer to codegen than a GitHub issue. |
| `session_dialogue` | bundled [`corpus.conversation.json`](./corpus.conversation.json) | Conversation-loop contracts |
| `harness_control` | bundled [`corpus.harness_control.json`](./corpus.harness_control.json) | CodingHarness smoke (Write/Edit + `write_root`) |
| `humaneval` | [OpenAI HumanEval](https://raw.githubusercontent.com/openai/human-eval/master/data/HumanEval.jsonl.gz) | Unit-coding floor only |
| `mbpp` | [Google sanitized MBPP](https://raw.githubusercontent.com/google-research/google-research/master/mbpp/sanitized-mbpp.json) | Unit-coding floor only |

Downloads cache under `{data_dir}/diagnostics/agent-evals/datasets/` (default limit 8, max 20).
Pull tries GitHub raw first, then a jsDelivr CDN mirror when raw is unreachable. You do not need to
pre-download datasets manually unless every mirror is blocked on your network.

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

- Workspace: `{data_dir}/diagnostics/agent-evals/workspaces/{run_id}/{case_id}/`
- `session.enabled = false` — no user conversation rows
- Not registered in `AgentRuntimeRegistry`
- `auto_approve = true`, `write_root` = eval workspace
- MCP / browser / computer-use / web search / memory distill / MoA off
- One in-flight run at a time (HTTP 409)

## Commands

Requires the `agent-eval` feature (example binary only; the library always builds):

```bash
# Scripted offline pass (no LLM)
cargo run -p nomi-agent-eval --example agent_eval --features agent-eval -- \
  demo --output /tmp/agent-eval-demo.jsonl

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

Each JSONL line stores `case_id`, `category`, `success`, scorer results, tokens, timing, and trajectory/artifact **counts**.
The full per-case trajectory and workspace artifacts are written beside JSONL as
`{data_dir}/diagnostics/agent-evals/runs/{run_id}/traces/{case_id}.json` (relative paths only; prompts and tool IO redacted).
Prompt text is sanitized with `nomi-redact` so `sk-…` secrets never land on disk.
Workspace absolute paths are never serialized.

The `/eval` page polls `GET /api/debug/agent-evals/runs/{id}` during a live run (`current_trace`) and loads a finished case via `GET …/cases/{case_id}/trace`.

The offline `OfflineDemoHarness` 100% pass rate is self-referential and is not a KPI for the live agent.

## Resume

`run` defaults to `--resume`: existing case_ids in the output JSONL are skipped.
Pass `--no-resume` to truncate and re-run. Live lab runs always start a new JSONL.

## Tests

```bash
cargo test -p nomi-agent-eval --all-targets
```
