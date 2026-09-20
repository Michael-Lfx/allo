/**
 * Agent Store AgentDefinition catalog client (`agent/list`, `agent/get`) over
 * the App Server WebSocket transport (docs/agent-store/05 §4.1), plus the
 * **definition export** face (`agent/export`, docs/agent-store/32).
 *
 * `list()` / `get()` are the catalog: structured fields only, and never a
 * persona. `export()` is the only way to obtain the definition itself, and it is
 * a separate method on purpose — the catalog is what every store UI calls while
 * browsing, so the export gate cannot live there.
 */

import type { Transport } from "./transport";
import type { AgentDetail, AgentSummary, ExpertPack } from "@flowy-agent-store/protocol";

export class AgentClient {
  constructor(private readonly transport: Transport) {}

  /** List Agent Store AgentDefinitions (public summaries only). */
  list(): Promise<AgentSummary[]> {
    return this.transport.request<AgentSummary[]>("agent/list", {});
  }

  /** Fetch one AgentDefinition; raw prompts never cross the wire. */
  get(agentId: string): Promise<AgentDetail> {
    return this.transport.request<AgentDetail>("agent/get", { agent_id: agentId });
  }

  /**
   * Export one AgentDefinition as a portable {@link ExpertPack}.
   *
   * Requires WebSocket: like the rest of this family, `agent/export` has no HTTP
   * route. The expert must be installed and enabled, or the host answers
   * `agent_not_installed` / `agent_disabled`; a host whose `[expert_export]`
   * table refuses the id answers `policy_denied`. Check
   * `capabilities.expert_export` for the *seam* — it does not promise this id is
   * exportable.
   */
  export(agentId: string): Promise<ExpertPack> {
    return this.transport.request<ExpertPack>("agent/export", { agent_id: agentId });
  }
}