/**
 * Agent Store Team catalog client (`team/list`, `team/get`) over the App
 * Server WebSocket transport (docs/agent-store/05 §4.2).
 */

import type { WebSocketTransport } from "./transport";
import type { TeamDetail, TeamSummary } from "./protocol";

export class TeamClient {
  constructor(private readonly transport: WebSocketTransport) {}

  /** List AgentTeamDefinitions (fixed-member rosters). */
  list(): Promise<TeamSummary[]> {
    return this.transport.request<TeamSummary[]>("team/list", {});
  }

  /** Fetch one TeamDefinition; planning-context bodies are never returned. */
  get(teamId: string): Promise<TeamDetail> {
    return this.transport.request<TeamDetail>("team/get", { team_id: teamId });
  }
}