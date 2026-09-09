/** ModelClient: `models/list` unwrapping + wire shape (no server needed). */
import { describe, expect, it } from "vitest";
import type { ModelList } from "@agent-store/protocol";
import { ModelClient } from "./models";
import type { NotificationListener, Transport } from "./transport";

class FakeTransport implements Transport {
  requests: { method: string; params: unknown }[] = [];
  response: ModelList = {
    items: [
      {
        provider_id: "prov-1",
        provider_name: "Demo Provider",
        model: "demo-model",
        display_name: "Demo Model",
        is_default: true,
      },
    ],
  };

  async connect(): Promise<void> {}
  async request<T>(method: string, params: unknown): Promise<T> {
    this.requests.push({ method, params });
    if (method !== "models/list") throw new Error(`unexpected method ${method}`);
    return this.response as unknown as T;
  }
  notify(): void {}
  onNotification(_listener: NotificationListener): () => void {
    return () => undefined;
  }
  close(): void {}
}

describe("ModelClient", () => {
  it("requests models/list and unwraps items", async () => {
    const transport = new FakeTransport();
    const client = new ModelClient(transport);

    const models = await client.list();

    expect(transport.requests).toEqual([{ method: "models/list", params: {} }]);
    expect(models).toHaveLength(1);
    expect(models[0]).toMatchObject({
      provider_id: "prov-1",
      model: "demo-model",
      is_default: true,
    });
  });
});
