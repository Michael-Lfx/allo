# Agent conversation evaluation

Offline / harness-backed evaluation for **session dialogue** agents.

## Corpus

- Manifest: [`corpus.conversation.json`](./corpus.conversation.json)
- Suite: `session_dialogue`
- Scorers are deterministic (substring / tool / turn / optional regex)

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

# Aggregate JSONL → summary JSON
cargo run -p nomi-agent-eval --example agent_eval --features agent-eval -- \
  summarize --input /tmp/agent-eval-demo.jsonl --output /tmp/agent-eval-summary.json
```

## Evidence hygiene

Each JSONL line stores `case_id`, `category`, `success`, scorer results, and timing.
Prompt text is sanitized with `nomi-redact` so `sk-…` secrets never land on disk.

## Resume

`run` defaults to `--resume`: existing case_ids in the output JSONL are skipped.
Pass `--no-resume` to truncate and re-run.

## Tests

```bash
cargo test -p nomi-agent-eval --all-targets
```
