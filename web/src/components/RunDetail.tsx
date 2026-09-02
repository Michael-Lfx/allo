import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { AppServerClient } from "../lib/client";
import { formatError } from "../lib/errors";
import { shortId } from "../ui/format";
import type { RunEvent, RunResult, RunView } from "../lib/protocol";

export function RunDetail({
  client,
  runId,
  onEvents,
}: {
  client: AppServerClient;
  runId: string;
  onEvents: (events: RunEvent[]) => void;
}) {
  const [view, setView] = useState<RunView | null>(null);
  const [result, setResult] = useState<RunResult | null>(null);
  const [events, setEvents] = useState<RunEvent[]>([]);
  const [resync, setResync] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [errorResult, setErrorResult] = useState<string | null>(null);
  const [busyCancel, setBusyCancel] = useState(false);
  const [busyCatchUp, setBusyCatchUp] = useState(false);
  const subscriptionRef = useRef<{ close: () => Promise<void> } | null>(null);

  const refresh = useCallback(async () => {
    try {
      setView(await client.runs.get(runId));
      setError(null);
    } catch (caught) {
      setError(formatError(caught));
    }
  }, [client, runId]);

  const fetchResult = useCallback(async () => {
    setErrorResult(null);
    try {
      setResult(await client.runs.result(runId));
    } catch (caught) {
      setErrorResult(formatError(caught));
    }
  }, [client, runId]);

  const catchUp = useCallback(async () => {
    setBusyCatchUp(true);
    try {
      const latest = await client.runs.events({ runId, limit: 200 });
      setEvents((existing) => mergeBySequence(existing, latest));
      onEvents(latest);
      setResync(null);
    } catch (caught) {
      setError(formatError(caught));
    } finally {
      setBusyCatchUp(false);
    }
  }, [client, runId, onEvents]);

  const cancel = useCallback(async () => {
    setBusyCancel(true);
    try {
      const expectedVersion = view?.version ?? 1;
      const cancelled = await client.runs.cancel({ runId, expectedVersion });
      setView(cancelled);
    } catch (caught) {
      setError(formatError(caught));
    } finally {
      setBusyCancel(false);
    }
  }, [client, runId, view]);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        if (!cancelled) {
          setView(await client.runs.get(runId));
        }
        const history = await client.runs.events({ runId, limit: 200 });
        if (cancelled) {
          return;
        }
        setEvents(history);
        onEvents(history);
        const subscription = await client.runs.follow(runId);
        if (cancelled) {
          void subscription.close();
          return;
        }
        subscriptionRef.current = subscription;
        subscription.onEvent((event) => {
          setEvents((existing) => mergeBySequence(existing, [event]));
          onEvents([event]);
          setError(null);
        });
        subscription.onResync((params) => {
          setResync(params.reason);
        });
        subscription.onError((subError) => {
          setError(formatError(subError));
        });
      } catch (caught) {
        if (!cancelled) {
          setError(formatError(caught));
        }
      }
    })();
    return () => {
      cancelled = true;
      void subscriptionRef.current?.close();
      subscriptionRef.current = null;
    };
  }, [client, runId, onEvents]);

  useEffect(() => {
    setEvents([]);
    setResult(null);
    setErrorResult(null);
    setResync(null);
  }, [runId]);

  const sortedEvents = useMemo(() => [...events].sort((a, b) => a.sequence - b.sequence), [events]);

  return (
    <section className="card">
      <h2>Run {shortId(runId)}</h2>
      <div className="row">
        <button onClick={refresh}>Refresh state</button>
        <button onClick={fetchResult}>Fetch result</button>
        <button onClick={catchUp} disabled={busyCatchUp}>
          {busyCatchUp ? "Catching up…" : "Catch up events"}
        </button>
        <button className="danger" onClick={cancel} disabled={busyCancel}>
          {busyCancel ? "Cancelling…" : "Cancel"}
        </button>
      </div>
      {error && <p className="error">{error}</p>}
      {resync && <p className="warn">resync required: {resync}</p>}
      {view && <pre className="view">{JSON.stringify(view, null, 2)}</pre>}
      {errorResult && <p className="error">{errorResult}</p>}
      {result && <pre className="view">{JSON.stringify(result, null, 2)}</pre>}

      <h3>Events ({sortedEvents.length})</h3>
      <table className="event-log">
        <thead>
          <tr>
            <th>seq</th>
            <th>type</th>
            <th>payload</th>
          </tr>
        </thead>
        <tbody>
          {sortedEvents.map((event) => (
            <tr key={`${event.run_id}:${event.sequence}`}>
              <td>{event.sequence}</td>
              <td>{event.event_type}</td>
              <td className="mono">{JSON.stringify(event.payload)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </section>
  );
}

function mergeBySequence(existing: RunEvent[], incoming: RunEvent[]): RunEvent[] {
  const seen = new Set(existing.map((event) => `${event.run_id}:${event.sequence}`));
  const merged = [...existing];
  for (const event of incoming) {
    const key = `${event.run_id}:${event.sequence}`;
    if (seen.has(key)) {
      continue;
    }
    seen.add(key);
    merged.push(event);
  }
  return merged;
}