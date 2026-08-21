import type { FloatingDockEntry } from "@oc/components/ui/aceternity/floating-dock";

import type { AddNodeMenuCommand, AddNodeMenuContext, ResolvedAddNodeMenuCommand, ToolCategory, ToolContext, ToolDefinition, ToolbarId, ToolbarPrefs } from "./tool-definition";

/** 模块级注册表 */
const registry = new Map<ToolbarId, ToolDefinition[]>();
const addNodeMenuRegistry: AddNodeMenuCommand[] = [];

/** 批量注册工具到指定工具栏（同 id 幂等覆盖，避免 HMR/重复导入叠出多份图标） */
export function registerToolbarTools(tools: ToolDefinition[]) {
    for (const tool of tools) {
        const previous = registry.get(tool.toolbar) ?? [];
        const list = previous.filter((item) => item.id !== tool.id);
        list.push(tool);
        registry.set(tool.toolbar, list);
    }
}

/** 注册添加节点菜单命令（同 id 幂等覆盖） */
export function registerAddNodeMenuCommands(commands: AddNodeMenuCommand[]) {
    for (const command of commands) {
        for (let index = addNodeMenuRegistry.length - 1; index >= 0; index -= 1) {
            if (addNodeMenuRegistry[index]?.id === command.id) addNodeMenuRegistry.splice(index, 1);
        }
        addNodeMenuRegistry.push(command);
    }
}

/** 获取某工具栏全部已注册工具（按 defaultOrder 升序；同 id 仅保留最后一次注册） */
export function getToolbarTools(toolbar: ToolbarId): ToolDefinition[] {
    const tools = registry.get(toolbar) ?? [];
    const deduped = new Map<string, ToolDefinition>();
    for (const tool of tools) deduped.set(tool.id, tool);
    return [...deduped.values()].sort((a, b) => a.defaultOrder - b.defaultOrder);
}

/** 获取添加节点菜单全部命令（按 defaultOrder 升序；同 id 仅保留最后一次注册） */
export function getAddNodeMenuCommands(): AddNodeMenuCommand[] {
    const deduped = new Map<string, AddNodeMenuCommand>();
    for (const command of addNodeMenuRegistry) deduped.set(command.id, command);
    return [...deduped.values()].sort((a, b) => a.defaultOrder - b.defaultOrder);
}

/** 默认偏好：全部工具按 defaultOrder 排列，全部可见 */
export function defaultToolbarPrefs(toolbar: ToolbarId): ToolbarPrefs {
    const tools = getToolbarTools(toolbar);
    return { order: tools.map((tool) => tool.id), hidden: [] };
}

/**
 * 解析工具栏条目——核心函数
 *
 * 流程：过滤 applicable → 应用用户排序 → 过滤用户隐藏 → 生成 FloatingDockEntry（含 separator 分组）
 */
export function resolveToolbarEntries(toolbar: ToolbarId, ctx: ToolContext, prefs: ToolbarPrefs | null): FloatingDockEntry[] {
    const tools = resolveToolbarTools(toolbar, ctx, prefs);
    return buildEntriesWithSeparators(tools, ctx);
}

/**
 * 解析工具栏工具——返回过滤排序后的 ToolDefinition[]（不含 separator）。
 * 供需要后处理工具列表的场景使用（如节点悬停工具栏合并图片工具）。
 */
export function resolveToolbarTools(toolbar: ToolbarId, ctx: ToolContext, prefs: ToolbarPrefs | null): ToolDefinition[] {
    const allTools = getToolbarTools(toolbar);
    const applicableTools = allTools.filter((tool) => !tool.applicable || tool.applicable(ctx));
    const effectivePrefs = prefs ?? defaultToolbarPrefs(toolbar);
    const hiddenSet = new Set(effectivePrefs.hidden);
    const visibleTools = applicableTools.filter((tool) => !hiddenSet.has(tool.id));
    const orderIndex = new Map(effectivePrefs.order.map((id, index) => [id, index]));
    return [...visibleTools].sort((a, b) => {
        const ai = orderIndex.has(a.id) ? orderIndex.get(a.id)! : Number.MAX_SAFE_INTEGER;
        const bi = orderIndex.has(b.id) ? orderIndex.get(b.id)! : Number.MAX_SAFE_INTEGER;
        if (ai !== bi) return ai - bi;
        return a.defaultOrder - b.defaultOrder;
    });
}

/** 解析添加节点菜单命令——applicable 过滤，并将 label/badge 解析为具体字符串 */
export function resolveAddNodeMenuCommands(ctx: AddNodeMenuContext): ResolvedAddNodeMenuCommand[] {
    return getAddNodeMenuCommands()
        .filter((command) => !command.applicable || command.applicable(ctx))
        .map((command) => ({
            ...command,
            label: typeof command.label === "function" ? command.label() : command.label,
            badge: command.badge === undefined ? undefined : typeof command.badge === "function" ? command.badge() : command.badge,
        }));
}

/**
 * 将工具列表转为 FloatingDockEntry，按 category 边界自动插入 separator。
 * danger 类工具会被包裹在 is-danger-group 容器中实现视觉隔离。
 */
function buildEntriesWithSeparators(tools: ToolDefinition[], ctx: ToolContext): FloatingDockEntry[] {
    const entries: FloatingDockEntry[] = [];
    let prevCategory: ToolCategory | null = null;
    let separatorIndex = 0;
    for (const tool of tools) {
        if (prevCategory !== null && prevCategory !== tool.category) {
            entries.push({ kind: "separator", id: `sep-${tool.toolbar}-${separatorIndex}` });
            separatorIndex += 1;
        }
        entries.push(toolToEntry(tool, ctx));
        prevCategory = tool.category;
    }
    return entries;
}

function toolToEntry(tool: ToolDefinition, ctx: ToolContext): FloatingDockEntry {
    return {
        kind: "command",
        id: tool.id,
        label: resolveText(tool.label, ctx),
        displayLabel: tool.displayLabel ? resolveText(tool.displayLabel, ctx) : undefined,
        icon: resolveIcon(tool.icon, ctx),
        active: tool.active?.(ctx),
        disabled: tool.disabled?.(ctx),
        danger: tool.danger,
        expands: tool.expands,
        onClick: (event) => tool.run(ctx, event),
    };
}

function resolveText(value: string | ((ctx: ToolContext) => string), ctx: ToolContext): string {
    return typeof value === "function" ? value(ctx) : value;
}

function resolveIcon(value: ToolDefinition["icon"], ctx: ToolContext) {
    return typeof value === "function" ? value(ctx) : value;
}
