/**
 * W12 skill write face (`16` R17): the store behind the WebUI's skill
 * management surface — list by origin, create / edit / delete / copy.
 *
 * The rules this store exists to keep:
 *
 * - **Nothing is optimistic.** Every action's landing spot is the server's own
 *   re-read (`skill/create|update|copy` answer with `skill/get`'s view,
 *   `skill/delete` with what the id now resolves to). The store never invents a
 *   value the host did not confirm, and the caller refreshes its list
 *   afterwards — the store does not splice results into a cached array.
 * - **Read-only is a first-class answer, not a hidden button.** The server
 *   enforces origin ownership with `policy_denied`; the store surfaces that
 *   message verbatim so a refused write is visible and explains itself.
 * - **One write at a time.** `busy` is a skill id (or `"create"`), so a second
 *   click cannot race the first, and the UI can disable exactly the row in
 *   flight.
 */

import { create } from "zustand";

import type {
  SkillCreateInput,
  SkillDeleteResult,
  SkillDetail,
  SkillUpdateInput,
} from "../lib/client";
import { formatError } from "../lib/errors";

/** The four host-only calls this store needs (the WebUI client satisfies it). */
export interface SkillAdminClient {
  createSkill: (input: SkillCreateInput) => Promise<SkillDetail>;
  updateSkill: (input: SkillUpdateInput) => Promise<SkillDetail>;
  deleteSkill: (skillId: string) => Promise<SkillDeleteResult>;
  copySkill: (skillId: string, newName: string) => Promise<SkillDetail>;
}

/** What the store reports after a successful write. */
export interface SkillAdminOutcome {
  /** `create` | `update` | `delete` | `copy`. */
  action: "create" | "update" | "delete" | "copy";
  /** The skill id the action was applied to (the new one, for create/copy). */
  skillId: string;
  /** `skill/delete` only: what the id resolves to now (`null` = nothing). */
  revealedOrigin?: string | null;
}

export interface SkillAdminState {
  /** Id of the skill being written (or `"create"`); `null` = idle. */
  busy: string | null;
  /** Last failure: an i18n key or the server's own `code: message`. */
  error: string | null;
  /** Last confirmed outcome; cleared when a new action starts. */
  outcome: SkillAdminOutcome | null;

  create: (client: SkillAdminClient | null, input: SkillCreateInput) => Promise<boolean>;
  update: (client: SkillAdminClient | null, input: SkillUpdateInput) => Promise<boolean>;
  remove: (client: SkillAdminClient | null, skillId: string) => Promise<boolean>;
  copy: (client: SkillAdminClient | null, skillId: string, newName: string) => Promise<boolean>;
  reset: () => void;
}

export const useSkillAdmin = create<SkillAdminState>()((set, get) => {
  /** Shared prologue: one write at a time, no fabricated result. */
  const begin = (key: string): boolean => {
    if (get().busy) return false;
    set({ busy: key, error: null, outcome: null });
    return true;
  };

  const fail = (caught: unknown): false => {
    set({ busy: null, error: formatError(caught) });
    return false;
  };

  return {
    busy: null,
    error: null,
    outcome: null,

    create: async (client, input) => {
      if (!begin("create")) return false;
      if (!client) return fail(new Error("settings.providerOffline"));
      try {
        const detail = await client.createSkill(input);
        set({
          busy: null,
          error: null,
          outcome: { action: "create", skillId: detail.id },
        });
        return true;
      } catch (caught) {
        return fail(caught);
      }
    },

    update: async (client, input) => {
      if (!begin(input.skill_id)) return false;
      if (!client) return fail(new Error("settings.providerOffline"));
      try {
        const detail = await client.updateSkill(input);
        set({
          busy: null,
          error: null,
          outcome: { action: "update", skillId: detail.id },
        });
        return true;
      } catch (caught) {
        return fail(caught);
      }
    },

    remove: async (client, skillId) => {
      if (!begin(skillId)) return false;
      if (!client) return fail(new Error("settings.providerOffline"));
      try {
        const result = await client.deleteSkill(skillId);
        set({
          busy: null,
          error: null,
          outcome: {
            action: "delete",
            skillId: result.skill_id,
            // `undefined` and `null` both mean "nothing visible there now"; the
            // UI says so instead of inventing an origin.
            revealedOrigin: result.revealed_origin ?? null,
          },
        });
        return true;
      } catch (caught) {
        return fail(caught);
      }
    },

    copy: async (client, skillId, newName) => {
      if (!begin(skillId)) return false;
      if (!client) return fail(new Error("settings.providerOffline"));
      try {
        const detail = await client.copySkill(skillId, newName);
        set({
          busy: null,
          error: null,
          outcome: { action: "copy", skillId: detail.id },
        });
        return true;
      } catch (caught) {
        return fail(caught);
      }
    },

    reset: () => set({ busy: null, error: null, outcome: null }),
  };
});
