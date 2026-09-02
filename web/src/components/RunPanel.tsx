import { useState } from "react";
import type { AppServerClient } from "../lib/client";
import type { RunReceipt } from "../lib/protocol";
import { formatError } from "../App";

const DEFAULT_AGENT_ID = "0190f5fe-7c00-7a00-8000-000000000004";

export function RunPanel({
  client,
  workspaceId,
  onRunStarted,
}: {
  client: AppServerClient;
  workspaceId?: string;
  onRunStarted: (runId: string) => void;
}) {
  const [agentId, setAgentId] = useState(DEFAULT_AGENT_ID);
  const [goal, setGoal] = useState("inspect the repository and report findings");
  const [idempotencyKey, setIdempotencyKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [receipt, setReceipt] = useState<RunReceipt | null>(null);
  const [error, setError] = useState<string | null>(null);

  const startRun = async () => {
    setBusy(true);
    setError(null);
    setReceipt(null);
    try {
      const result = await client.runs.agent({
        agentId: agentId.trim(),
        goal: goal.trim(),
        workspaceId,
        idempotencyKey: idempotencyKey.trim() || undefined,
      });
      setReceipt(result);
      onRunStarted(result.run_id);
    } catch (caught) {
      setError(formatError(caught));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="card">
      <h2>Agent Run</h2>
      <div className="grid">
        <label>
          Agent ID (AgentDefinition)
          <input value={agentId} onChange={(event) => setAgentId(event.target.value)} spellCheck={false} />
        </label>
        <label>
          Goal
          <input value={goal} onChange={(event) => setGoal(event.target.value)} />
        </label>
        <label>
          Idempotency key (optional)
          <input value={idempotencyKey} onChange={(event) => setIdempotencyKey(event.target.value)} spellCheck={false} />
        </label>
      </div>
      <div className="row">
        <button onClick={startRun} disabled={busy || !agentId.trim() || !goal.trim()}>
          {busy ? "Starting…" : "Start run"}
        </button>
        <span className="muted">workspace: {workspaceId ?? "none"}</span>
      </div>
      {error && <p className="error">{error}</p>}
      {receipt && (
        <pre className="receipt">{JSON.stringify(receipt, null, 2)}</pre>
      )}
    </section>
  );
}