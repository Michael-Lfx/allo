/**
 * W12 技能写入面（`16` R17）：WebUI 技能管理界面背后的 store——按来源列出、
 * 创建 / 编辑 / 删除 / 复制。
 *
 * 本 store 旨在守住的规则：
 *
 * - **不做任何乐观处理。** 每个动作的落点都是服务端自身的回读（`skill/create|update|copy`
 *   以 `skill/get` 的视图作答，`skill/delete` 以该 id 现在解析到的结果作答）。store
 *   绝不臆造宿主未确认的值，调用方随后自行刷新列表——store 不会把结果拼接到
 *   缓存数组里。
 * - **只读是一种一等公民式的回应，而非一个隐藏按钮。** 服务端用 `policy_denied`
 *   强制来源归属；store 原样暴露该消息，使被拒绝的写入可见且能自我说明。
 * - **同一时刻只写一个。** `busy` 是某个技能 id（或 `"create"`），因此第二次点击
 *   不会与第一次竞态，UI 也能精确禁用正在写入的那一行。
 */

import { create } from "zustand";

import type {
  SkillCreateInput,
  SkillDeleteResult,
  SkillDetail,
  SkillUpdateInput,
} from "../lib/client";
import { formatError } from "../lib/errors";

/** 本 store 需要的、仅宿主侧的四个调用（由 WebUI 客户端满足）。 */
export interface SkillAdminClient {
  createSkill: (input: SkillCreateInput) => Promise<SkillDetail>;
  updateSkill: (input: SkillUpdateInput) => Promise<SkillDetail>;
  deleteSkill: (skillId: string) => Promise<SkillDeleteResult>;
  copySkill: (skillId: string, newName: string) => Promise<SkillDetail>;
}

/** store 在一次成功写入后汇报的内容。 */
export interface SkillAdminOutcome {
  /** `create` | `update` | `delete` | `copy`。 */
  action: "create" | "update" | "delete" | "copy";
  /** 动作所作用的技能 id（create/copy 时为新建的那个）。 */
  skillId: string;
  /** 仅 `skill/delete`：该 id 现在解析到的结果（`null` = 无）。 */
  revealedOrigin?: string | null;
}

export interface SkillAdminState {
  /** 正在被写入的技能 id（或 `"create"`）；`null` = 空闲。 */
  busy: string | null;
  /** 最近一次失败：i18n 键或服务端自身的 `code: message`。 */
  error: string | null;
  /** 最近一次已确认的结果；新动作启动时清空。 */
  outcome: SkillAdminOutcome | null;

  create: (client: SkillAdminClient | null, input: SkillCreateInput) => Promise<boolean>;
  update: (client: SkillAdminClient | null, input: SkillUpdateInput) => Promise<boolean>;
  remove: (client: SkillAdminClient | null, skillId: string) => Promise<boolean>;
  copy: (client: SkillAdminClient | null, skillId: string, newName: string) => Promise<boolean>;
  reset: () => void;
}

export const useSkillAdmin = create<SkillAdminState>()((set, get) => {
  /** 共享前奏：同一时刻只写一个，不臆造结果。 */
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
            // `undefined` 与 `null` 都表示“那里现在什么都没有可见的”；
            // UI 会如实说明，而非臆造一个来源。
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
