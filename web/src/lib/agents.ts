/**
 * Agent Store AgentDefinition catalog client (`agent/list`, `agent/get`) over
 * the App Server WebSocket transport (docs/agent-store/05 §4.1). Never a
 * Runtime Agent instance; structured fields only.
 */

import type { WebSocketTransport } from "./transport";
import type { AgentDetail, AgentSummary } from "./protocol";

export class AgentClient {
  constructor(private readonly transport: WebSocketTransport) {}

  /** List Agent Store AgentDefinitions (public summaries only). */
  list(): Promise<AgentSummary[]> {
    return this.transport.request<AgentSummary[]>("agent/list", {});
  }

  /** Fetch one AgentDefinition; raw prompts never cross the wire. */
  get(agentId: string): Promise<AgentDetail> {
    return this.transport.request<AgentDetail>("agent/get", { agent_id: agentId });
  }
}