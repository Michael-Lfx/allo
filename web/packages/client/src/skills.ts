/**
 * Agent Store Skill catalog client (`skill/list`, `skill/get`) over the
 * App Server WebSocket transport, plus the Skill **file tree** read face
 * (`skill/files`, `skill/file`, doc 24 §4).
 */

import type { Transport } from "./transport";
import type {
  SkillDetail,
  SkillFileContent,
  SkillFileList,
  SkillSummary,
} from "@flowy-agent-store/protocol";

export class SkillClient {
  constructor(private readonly transport: Transport) {}

  /** List the Agent Store Skill catalog (public summaries only). */
  list(): Promise<SkillSummary[]> {
    return this.transport.request<SkillSummary[]>("skill/list", {});
  }

  /** Fetch one Skill detail; instructions are a bounded public summary. */
  get(skillId: string): Promise<SkillDetail> {
    return this.transport.request<SkillDetail>("skill/get", { skill_id: skillId });
  }

  /**
   * Every readable file inside a Skill's directory, with a tree digest.
   *
   * This is the only way to learn what a Skill actually ships: `get()` returns
   * a bounded summary of `SKILL.md` and says nothing about the `references/`,
   * `scripts/`, `templates/` and `assets/` beside it. Check
   * `capabilities.skill_files` first — a host may wire the catalog without
   * this face, in which case it answers `unsupported_operation`.
   */
  files(skillId: string): Promise<SkillFileList> {
    return this.transport.request<SkillFileList>("skill/files", { skill_id: skillId });
  }

  /**
   * One skill file's bytes, over the WebSocket binding.
   *
   * The wire carries base64 because JSON has no byte string, so this decodes
   * back to `Uint8Array` for you. The HTTP route
   * (`GET /api/app-server/skills/{id}/files/{path}`) returns the raw body
   * instead, but that is deliberately not on the typed client's JSON
   * transport — use this method, or `fetch` the route directly if you are on a
   * host token.
   */
  async readFile(skillId: string, path: string): Promise<Uint8Array> {
    const file = await this.transport.request<SkillFileContent>("skill/file", {
      skill_id: skillId,
      path,
    });
    return decodeBase64(file.content);
  }

  /** Same as {@link readFile}, but also returns the server's content type. */
  readFileWithType(skillId: string, path: string): Promise<SkillFileContent> {
    return this.transport.request<SkillFileContent>("skill/file", {
      skill_id: skillId,
      path,
    });
  }
}

/**
 * Decode base64 to bytes.
 *
 * `atob` is used when present (browsers, Bun, Node 16+) and a manual decode
 * otherwise, so the package keeps working on a bare Node without a DOM lib.
 */
function decodeBase64(content: string): Uint8Array {
  if (typeof atob === "function") {
    const binary = atob(content);
    const bytes = new Uint8Array(binary.length);
    for (let index = 0; index < binary.length; index += 1) {
      bytes[index] = binary.charCodeAt(index);
    }
    return bytes;
  }
  // Node without `atob`: `Buffer` is available but untyped here.
  const bufferCtor = (globalThis as { Buffer?: { from(input: string, encoding: string): Uint8Array } })
    .Buffer;
  if (bufferCtor) return bufferCtor.from(content, "base64");
  throw new Error("no base64 decoder available in this environment");
}