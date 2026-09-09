/** AgentRunHandle: Codex-parity launch → await finished → iterate events. */
import { describe, expect, it, vi } from "vitest";
import { RunClient } from "./runs";
import { launchRun, type AgentRunHandle } from "./run-handle";
import type { RunEvent, ServerNotification } from "@flowy-agent-store/protocol";
import type { NotificationListener, Transport } from "./transport";

const RUN_ID = "0190f5fe-run-handle-0000-000000000001";

function runEvent(sequence: number, event_type = "run.status"): RunEvent {
  return { run_id: RUN_ID, sequence, event_type, payload: {} };
}

function liveEvent(sequence: number): ServerNotification {
  return { jsonrpc: "2.0", method: "event", params: runEvent(sequence) } as ServerNotification;
}

class FakeTransport implements Transport {
  eventsByCursor: RunEvent[] = [];
  failEvents = false;
  private listeners = new Set<NotificationListener>();

  async connect(): Promise<void> {}
  async request<T>(method: string, params: unknown): Promise<T> {
    if (method === "agent/run") {
      return { run_id: RUN_ID, status: "planning", version: 0, preset_revision: 1, content_digest: "sha256:x" } as unknown as T;
    }
    if (method === "run/get") {
      const status = (params as { status?: string }).status ?? this.getStatus();
      return { run_id: RUN_ID, status, version: 1, summary: null, output_files: [] } as unknown as T;
    }
    if (method === "run/subscribe" || method === "run/unsubscribe") {
      return { subscribed: true } as unknown as T;
    }
    if (method === "run/cancel") {
      return { run_id: RUN_ID, status: "cancelling", version: 2, summary: null, output_files: [] } as unknown as T;
    }
    if (method === "run/result") {
      return {
        run_id: RUN_ID,
        status: "completed",
        version: 6,
        summary: "done: 3 steps",
        output_files: ["report.md"],
        preset_revision: 1,
        content_digest: "sha256:x",
      } as unknown as T;
    }
    if (method === "run/events") {
      if (this.failEvents) {
        throw new Error("events unavailable");
      }
      const after = (params as { after_sequence?: number }).after_sequence ?? 0;
      return this.eventsByCursor.filter((e) => e.sequence > after) as unknown as T;
    }
    throw new Error(`unexpected method ${method}`);
  }
  notify(): void {}
  onNotification(listener: NotificationListener): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }
  close(): void {}
  emit(notification: ServerNotification): void {
    for (const listener of [...this.listeners]) {
      listener(notification);
    }
  }
  getStatus(): string {
    return "planning";
  }
  setStatus(_status: string): void {}
}

describe("AgentRunHandle", () => {
  it("finished resolves with the aggregated terminal turn result", async () => {
    const transport = new FakeTransport();
    transport.eventsByCursor = [runEvent(1, "run.started"), runEvent(2, "attempt.updated")];
    // run/get flips to completed after the first poll.
    vi.spyOn(transport, "getStatus").mockReturnValueOnce("planning").mockReturnValue("completed");
    const handle = await launchRun(new RunClient(transport), { agentId: "", goal: "hi" });
    const result = await handle.finished;
    expect(result.status).toBe("completed");
    expect(result.final_response).toBe("done: 3 steps");
    expect(result.output_files).toEqual(["report.md"]);
    expect(result.preset_revision).toBe(1);
    expect(result.items.map((item) => item.kind)).toEqual(["other", "attempt"]);
    expect(result.events.map((event) => event.sequence)).toEqual([1, 2]);
    await handle.close();
  });

  it("async iteration delivers live events in order", async () => {
    const transport = new FakeTransport();
    vi.spyOn(transport, "getStatus").mockReturnValue("planning");
    const handle = await launchRun(new RunClient(transport), { agentId: "", goal: "hi" });
    const seen: number[] = [];
    const consume = (async () => {
      for await (const event of handle) {
        seen.push(event.sequence);
        if (seen.length >= 3) {
          await handle.close();
          break;
        }
      }
    })();
    transport.emit(liveEvent(1));
    transport.emit(liveEvent(2));
    transport.emit(liveEvent(2)); // duplicate dropped by subscription
    transport.emit(liveEvent(3));
    await consume;
    expect(seen).toEqual([1, 2, 3]);
    await handle.close();
  });

  it("events emitted before iteration starts are buffered", async () => {
    const transport = new FakeTransport();
    vi.spyOn(transport, "getStatus").mockReturnValue("planning");
    const handle: AgentRunHandle = await launchRun(new RunClient(transport), { agentId: "", goal: "hi" });
    transport.emit(liveEvent(1));
    transport.emit(liveEvent(2));
    const seen: number[] = [];
    for await (const event of handle) {
      seen.push(event.sequence);
      if (seen.length >= 2) {
        await handle.close();
        break;
      }
    }
    expect(seen).toEqual([1, 2]);
    await handle.close();
  });

  it("cancel forwards expected_version from the receipt", async () => {
    const transport = new FakeTransport();
    vi.spyOn(transport, "getStatus").mockReturnValue("planning");
    const spy = vi.spyOn(transport, "request");
    const handle = await launchRun(new RunClient(transport), { agentId: "", goal: "hi" });
    await handle.cancel();
    const cancel = spy.mock.calls.find(([method]) => method === "run/cancel");
    expect(cancel?.[1]).toMatchObject({ run_id: RUN_ID, expected_version: 0 });
    await handle.close();
  });
});
