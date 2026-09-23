/**
 * `ConnectorClient.waitForAuth`: the OAuth wait that says why it ended.
 *
 * The class takes a `Transport`, so these tests drive a stub that answers
 * `connector/auth/status` from a script — no WebSocket, no server. What is worth
 * testing here is the loop's contract: it stops the moment the host reports a
 * reason (instead of burning its whole budget and reporting "never became
 * authenticated"), it reports `authenticated` as soon as it is true, it bounds
 * itself, and a failing status *read* still rejects.
 */
import { describe, expect, it } from "vitest";
import type { OAuthStatusView } from "@flowy-agent-store/protocol";
import { ConnectorClient } from "./connectors";
import type { Transport } from "./transport";

function clientAnswering(answers: OAuthStatusView[]): { client: ConnectorClient; reads: () => number } {
  let reads = 0;
  const transport = {
    async request<T>(method: string): Promise<T> {
      if (method !== "connector/auth/status") throw new Error(`unexpected method ${method}`);
      const answer = answers[Math.min(reads, answers.length - 1)];
      reads += 1;
      return answer as unknown as T;
    },
  } as unknown as Transport;
  return { client: new ConnectorClient(transport), reads: () => reads };
}

describe("ConnectorClient.waitForAuth", () => {
  it("reports the reason the moment the host gives one", async () => {
    const { client, reads } = clientAnswering([
      { state: "not_authenticated", error: null },
      {
        state: "not_authenticated",
        error: "OAuth error: authorization server is throttling OAuth requests; retry in 60s",
      },
    ]);

    const outcome = await client.waitForAuth("conn-1", { timeoutMs: 5_000, pollMs: 1 });

    expect(outcome).toEqual({
      state: "error",
      error: "OAuth error: authorization server is throttling OAuth requests; retry in 60s",
    });
    // It must not keep polling a flow that is over.
    expect(reads()).toBe(2);
  });

  it("returns immediately when the flow already finished", async () => {
    const { client, reads } = clientAnswering([{ state: "authenticated" }]);

    await expect(client.waitForAuth("conn-1", { timeoutMs: 5_000, pollMs: 1 })).resolves.toEqual({
      state: "authenticated",
    });
    expect(reads()).toBe(1);
  });

  it("bounds itself and says so", async () => {
    const { client } = clientAnswering([{ state: "not_authenticated", error: null }]);

    await expect(client.waitForAuth("conn-1", { timeoutMs: 20, pollMs: 5 })).resolves.toEqual({
      state: "timeout",
    });
  });

  it("rejects when the status read itself fails", async () => {
    const transport = {
      async request(): Promise<never> {
        throw new Error("transport closed");
      },
    } as unknown as Transport;

    await expect(new ConnectorClient(transport).waitForAuth("conn-1")).rejects.toThrow(
      "transport closed",
    );
  });
});
