/**
 * W1b 命令面板 —— `/` 模式只保留引擎侧命令 `/compact`。
 *
 * 面板的交互（焦点始终留在 textarea、IME 守卫、↑↓ 光标、Esc 清半截触发符）仍在
 * `Composer.tsx`；这里只回答「该列哪些行、怎么过滤、选中后要做什么」，因此可以单测。
 *
 * `/compact` 不是前端行为：它作为一条普通消息发出去，由后端 nomi 引擎在**任何 LLM
 * 调用之前**拦截并压缩会话上下文（与桌面端同一套引擎命令）。
 */

/** 面板行的语义分类（图标与分组由组件决定）。连接器**不是**面板行：它换的是
 *  宿主的工具面，改在 `+` 菜单里以开关呈现（doc `28`）。 */
export type PaletteItemKind = "compact" | "agent" | "skill";

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
  /** 选中后执行的语义 id。 */
  actionId?: string;
  disabled?: boolean;
  /** 禁用原因（i18n key）；禁用行不吞掉点击，而是明确说明为什么不可用。 */
  disabledReasonKey?: string;
}

/** 面板文案解析器（注入 `t`，模块本身不依赖 i18n 实例）。 */
export type PaletteTranslate = (key: string, params?: Record<string, unknown>) => string;

export const PALETTE_GROUP_KEYS = {
  command: "palette.groupCommands",
  mention: "palette.groupMentions",
} as const;

/**
 * `/` 模式的唯一一行：`/compact`。
 *
 * 没有选中会话时仍然显示，但禁用并说明原因——压缩需要一个会话才能落到历史上。
 */
export function compactPaletteRows(hasConversation: boolean, t: PaletteTranslate): PaletteItem[] {
  const disabled = !hasConversation;
  return [
    {
      id: "compact",
      kind: "compact",
      actionId: "compact",
      groupKey: PALETTE_GROUP_KEYS.command,
      label: t("palette.compact"),
      hint: "/compact",
      disabled,
      disabledReasonKey: disabled ? "palette.needsConversation" : undefined,
    },
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
