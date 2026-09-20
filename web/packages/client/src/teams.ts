/**
 * Agent Store Team catalog client (`team/list`, `team/get`) over the App Server
 * WebSocket transport (docs/agent-store/05 §4.2), plus the definition export
 * face (`team/export`, docs/agent-store/32).
 */

import type { Transport } from "./transport";
import type { ExpertPack, TeamDetail, TeamSummary } from "@flowy-agent-store/protocol";

export class TeamClient {
  constructor(private readonly transport: Transport) {}

  /** List AgentTeamDefinitions (fixed-member rosters). */
  list(): Promise<TeamSummary[]> {
    return this.transport.request<TeamSummary[]>("team/list", {});
  }

  /** Fetch one TeamDefinition; planning-context bodies are never returned. */
  get(teamId: string): Promise<TeamDetail> {
    return this.transport.request<TeamDetail>("team/get", { team_id: teamId });
  }

  /**
   * Export one TeamDefinition as a portable {@link ExpertPack}, with every member
   * expanded and the **leader first**.
   *
   * All-or-nothing: if any member is uninstalled, disabled, or refused by the
   * host's `[expert_export]` table, the whole call fails rather than returning a
   * partial roster — the same posture `team/run` takes before it creates
   * anything. `teamVersion` is an optional pin; a mismatch is `version_mismatch`.
   */
  export(teamId: string, teamVersion?: string): Promise<ExpertPack> {
    return this.transport.request<ExpertPack>("team/export", {
      team_id: teamId,
      team_version: teamVersion,
    });
  }
}