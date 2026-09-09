/**
 * Public model directory client (`models/list`, REQ-PAR-05b) over the App
 * Server WebSocket transport. The projection carries only provider identity
 * and model names — credentials never cross this surface.
 */

import type { Transport } from "./transport";
import type { ModelList, ModelSummary } from "@agent-store/protocol";

export class ModelClient {
  constructor(private readonly transport: Transport) {}

  /** Enabled providers' models; `is_default` marks the `agent/run` fallback. */
  async list(): Promise<ModelSummary[]> {
    const response = await this.transport.request<ModelList>("models/list", {});
    return response.items;
  }
}
