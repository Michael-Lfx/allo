/**
 * W1b 命令面板 V2（R19）—— 「会话动作 + 模型 / 思考等级」并入同一面板的数据层。
 *
 * 交互（焦点始终留在 textarea、IME 守卫、↑↓ 光标、Esc 清半截触发符）仍在
 * `Composer.tsx`；这里只回答「该列哪些行、怎么过滤、选中后要做什么」，因此可以单测。
 *
 * **刻意不做「归档」**：协议里没有归档 / 回收站（`11` §2.2 未做，`16` §3.5 方向四），
 * 面板里放一个点了没反应的「归档」比不放更糟——R19 的行文提到它，但落地按现状收敛。
 */

import type { ModelSummary, ProviderWithModel, ReasoningEffort } from "@flowy-agent-store/protocol";

/** 面板行的语义分类（图标与分组由组件决定）。 */
export type PaletteItemKind = "action" | "prompt" | "agent" | "skill" | "connector" | "session" | "model" | "effort";

export interface PaletteItem {
  /** Stable key; for catalog items this is the mention id. */
  id: string;
  kind: PaletteItemKind;
  /** 已本地化的行文案。 */
  label: string;
  hint?: string;
  /** 分组标题（i18n key）；同组行连续渲染，组件据此插入小标题。 */
  groupKey?: string;
  /** 额外可搜文本（模型 id / provider / 动作别名），不参与渲染。 */
  keywords?: string;
  /** `action` / `session` 行选中后执行的语义 id。 */
  actionId?: string;
  /** `model` 行：`provider/model` 选择键。 */
  modelKey?: string;
  /** `effort` 行：`""` 表示「默认」。 */
  effort?: ReasoningEffort | "";
  disabled?: boolean;
  /** 禁用原因（i18n key）；禁用行不吞掉点击，而是明确说明为什么不可用。 */
  disabledReasonKey?: string;
}

/** 面板文案解析器（注入 `t`，模块本身不依赖 i18n 实例）。 */
export type PaletteTranslate = (key: string, params?: Record<string, unknown>) => string;

export const PALETTE_GROUP_KEYS = {
  command: "palette.groupCommands",
  prompt: "palette.groupPrompts",
  session: "palette.groupSession",
  model: "palette.groupModels",
  effort: "palette.groupEfforts",
  mention: "palette.groupMentions",
} as const;

/** 思考等级从「默认」到「超高」，顺序固定（面板里不按字母排）。 */
export const EFFORT_LEVELS: readonly (ReasoningEffort | "")[] = ["", "low", "medium", "high", "xhigh", "max"];

const EFFORT_LABEL_KEYS: Record<string, string> = {
  "": "modelPicker.effortDefault",
  low: "modelPicker.effortLow",
  medium: "modelPicker.effortMedium",
  high: "modelPicker.effortHigh",
  xhigh: "modelPicker.effortXhigh",
  max: "modelPicker.effortMax",
};

export interface PaletteContext {
  /** 有选中会话才允许重命名 / 删除 / 分享；没有会话时行仍在，但被禁用并说明原因。 */
  hasConversation: boolean;
  models: ModelSummary[];
  selectedModelKey: string | null;
  currentModel: ProviderWithModel | null;
  /** 当前思考等级；`""` = 默认。 */
  currentEffort: string;
}

/**
 * 会话动作行：重命名 / 删除 / 分享 / 复制会话 ID。
 * 没有选中会话时全部禁用（仍然显示，让用户知道这些动作存在且为何不可用）。
 */
export function sessionPaletteRows(context: PaletteContext, t: PaletteTranslate): PaletteItem[] {
  const disabled = !context.hasConversation;
  const rows: Array<{ id: string; labelKey: string }> = [
    { id: "session.rename", labelKey: "palette.renameConversation" },
    { id: "session.delete", labelKey: "palette.deleteConversation" },
    { id: "session.share", labelKey: "palette.shareConversation" },
    { id: "session.copyId", labelKey: "palette.copySessionId" },
  ];
  return rows.map((row) => ({
    id: row.id,
    kind: "session",
    actionId: row.id,
    groupKey: PALETTE_GROUP_KEYS.session,
    label: t(row.labelKey),
    keywords: row.id.split(".")[1],
    disabled,
    disabledReasonKey: disabled ? "palette.needsConversation" : undefined,
  }));
}

/** 模型行：来自 `models/list` 的公开目录；当前选中项标「当前」。 */
export function modelPaletteRows(context: PaletteContext, t: PaletteTranslate): PaletteItem[] {
  const currentKey = context.selectedModelKey
    ?? (context.currentModel ? `${context.currentModel.provider_id}/${context.currentModel.model}` : null);
  return context.models.map((model) => {
    const key = `${model.provider_id}/${model.model}`;
    const name = model.display_name ?? model.model;
    const isCurrent = key === currentKey;
    return {
      id: `model.${key}`,
      kind: "model" as const,
      modelKey: key,
      groupKey: PALETTE_GROUP_KEYS.model,
      label: name,
      hint: isCurrent ? `${model.provider_name} · ${t("palette.current")}` : model.provider_name,
      keywords: `${model.provider_id} ${model.model} ${model.display_name ?? ""}${model.is_default ? " default" : ""}`,
    };
  });
}

/** 思考等级行：默认 + 五档；当前档位标「当前」。 */
export function effortPaletteRows(currentEffort: string, t: PaletteTranslate): PaletteItem[] {
  const current = currentEffort === "default" ? "" : currentEffort;
  return EFFORT_LEVELS.map((effort) => {
    const label = t(EFFORT_LABEL_KEYS[effort] ?? String(effort));
    const isCurrent = effort === current;
    return {
      id: `effort.${effort === "" ? "default" : effort}`,
      kind: "effort" as const,
      effort,
      groupKey: PALETTE_GROUP_KEYS.effort,
      label,
      hint: isCurrent ? t("palette.current") : undefined,
      keywords: effort,
    };
  });
}

/**
 * `Cmd/Ctrl+K` 与 `/` 的面板内容：命令 → 提示 → 会话 → 模型 → 思考等级。
 *
 * 顺序即优先级：用户按 Cmd+K 多数是想执行一个动作，模型 / 等级切换排在后面。
 */
export function commandPaletteRows(
  context: PaletteContext,
  t: PaletteTranslate,
  extras: { actions: PaletteItem[]; prompts: PaletteItem[] },
): PaletteItem[] {
  return [
    ...extras.actions,
    ...extras.prompts,
    ...sessionPaletteRows(context, t),
    ...modelPaletteRows(context, t),
    ...effortPaletteRows(context.currentEffort, t),
  ];
}

/** 空查询返回全量；否则按行文案 + `keywords` + hint 做大小写不敏感的子串匹配。 */
export function filterPaletteItems(items: PaletteItem[], query: string): PaletteItem[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return items;
  return items.filter((item) => {
    const haystack = `${item.label} ${item.keywords ?? ""} ${item.hint ?? ""}`.toLowerCase();
    return haystack.includes(needle);
  });
}
