/**
 * Agent Store Skill catalog client (`skill/list`, `skill/get`) over the
 * App Server WebSocket transport.
 */

import type { WebSocketTransport } from "./transport";
import type { SkillDetail, SkillSummary } from "./protocol";

export class SkillClient {
  constructor(private readonly transport: WebSocketTransport) {}

  /** List the Agent Store Skill catalog (public summaries only). */
  list(): Promise<SkillSummary[]> {
    return this.transport.request<SkillSummary[]>("skill/list", {});
  }

  /** Fetch one Skill detail; instructions are a bounded public summary. */
  get(skillId: string): Promise<SkillDetail> {
    return this.transport.request<SkillDetail>("skill/get", { skill_id: skillId });
  }
}