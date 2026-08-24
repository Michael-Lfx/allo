import copyToClipboard from "copy-to-clipboard";
import { useMemo, useRef, useState } from "react";
import { Button, Segmented, Select, Tooltip } from "antd";
import { Copy, Settings2, Trash2, X } from "lucide-react";

import { ModelIcon } from "@oc/components/model-picker";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { imageReferenceLabel } from "@oc/lib/image-reference-prompt";
import { canvasThemes } from "@oc/lib/canvas-theme";
import { modelDisplayName, resolveModelChannel, selectableModelsByCapability, type AiConfig } from "@oc/stores/use-config-store";
import { useThemeStore } from "@oc/stores/use-theme-store";
import type { LocalUser } from "@oc/stores/use-user-store";
import { CanvasNodeType, type CanvasAssistantMessage, type CanvasAssistantReference, type CanvasAssistantSession, type CanvasNodeData } from "@oc/types/canvas";
import { AgentChatEmptyState } from "./canvas-agent-panel-chrome";
import { AgentChatMessage, AgentWorkingMessage, type CanvasAgentChatMessage } from "./canvas-agent-chat-ui";
import type { OnlineAgentLog } from "./canvas-online-agent-loop";

export type OnlineAgentLogContext = { model: string; running: boolean; confirmTools: boolean; messages: number; nodes: number; connections: number };

export function AgentTextModelPicker({ config, value, onChange }: { config: AiConfig; value: string; onChange: (model: string) => void }) {
    const options = useMemo(() => Array.from(new Set([value, ...selectableModelsByCapability(config, "text")].filter(Boolean))), [config, value]);
    const current = value || "";
    return (
        <div
            className="min-w-0 max-w-[240px]"
            data-canvas-no-zoom
            onMouseDown={(event) => event.stopPropagation()}
            onPointerDown={(event) => event.stopPropagation()}
            onWheel={(event) => event.stopPropagation()}
        >
            <Select<string>
                size="small"
                variant="borderless"
                value={current || undefined}
                className="agent-text-model-select w-full"
                popupMatchSelectWidth={288}
                listHeight={280}
                getPopupContainer={() => document.body}
                classNames={{ popup: { root: "agent-text-model-select-dropdown" } }}
                options={options.map((model) => ({ value: model, label: `${modelDisplayName(config, model)} ${resolveModelChannel(config, model).name}` }))}
                notFoundContent={<span className="block py-2 text-center text-xs text-foreground/48">{canvasT("videoCanvas.agent.noTextModels", "暂无文本模型")}</span>}
                optionRender={(option) => {
                    const model = String(option.value);
                    return <span className="flex min-w-0 items-center gap-2"><ModelIcon config={config} model={model} /><span className="min-w-0 flex-1 truncate">{modelDisplayName(config, model)}</span><span className="shrink-0 text-xs opacity-55">{resolveModelChannel(config, model).name}</span></span>;
                }}
                labelRender={() => <span className="flex min-w-0 items-center gap-1.5"><ModelIcon config={config} model={current} /><span className="min-w-0 truncate">{current ? modelDisplayName(config, current) : canvasT("videoCanvas.agent.selectTextModel", "选择文本模型")}</span>{current ? <span className="shrink-0 opacity-55">{resolveModelChannel(config, current).name}</span> : null}</span>}
                onChange={onChange}
                aria-label={canvasT("videoCanvas.agent.selectAgentModelAria", "选择 Agent 文本模型")}
                title={current ? `${modelDisplayName(config, current)} · ${resolveModelChannel(config, current).name}` : canvasT("videoCanvas.agent.selectTextModel", "选择文本模型")}
            />
        </div>
    );
}

export function AssistantHistory({
    sessions,
    activeSession,
    onOpen,
    onDelete,
}: {
    sessions: CanvasAssistantSession[];
    activeSession: CanvasAssistantSession | null;
    onOpen: (id: string) => void;
    onDelete: (id: string) => void;
}) {
    const theme = canvasThemes[useThemeStore((state) => state.theme)];

    return (
        <div className="space-y-3">
            <div className="text-sm" style={{ color: theme.node.muted }}>
                {sessions.length ? canvasT("videoCanvas.agent.historyCount", "{{count}} 条历史", { count: sessions.length }) : canvasT("videoCanvas.agent.noHistory", "暂无历史")}
            </div>
            {sessions.map((session) => (
                <div key={session.id} className="rounded-md px-2.5 py-2 transition-colors" style={{ background: session.id === activeSession?.id ? theme.accent.primarySoft : "transparent", color: theme.node.text }}>
                    <div className="flex items-center gap-2">
                        <div className="min-w-0 flex-1">
                            <div className="flex min-w-0 items-center gap-1.5">
                                {session.id === activeSession?.id ? <span className="shrink-0 text-[var(--fs-tiny)] font-medium" style={{ color: theme.node.text }}>{canvasT("videoCanvas.agent.current", "当前")}</span> : null}
                                <div className="truncate text-sm font-medium leading-5">{session.title}</div>
                            </div>
                            <div className="truncate text-[var(--fs-label)] leading-4 opacity-65">{sessionPreview(session)}</div>
                        </div>
                        <div className="flex shrink-0 items-center gap-1">
                            <span className="text-[var(--fs-tiny)] opacity-55">{formatSessionTime(session.updatedAt || session.createdAt)}</span>
                            <Button size="small" className="!h-6 !px-2" onClick={() => onOpen(session.id)}>
                                {canvasT("videoCanvas.agent.enter", "进入")}
                            </Button>
                            <Tooltip title={canvasT("videoCanvas.agent.deleteRecord", "删除记录")}>
                                <Button size="small" danger type="text" className="!h-6 !w-6 !min-w-6" icon={<Trash2 className="size-3.5" />} onClick={() => onDelete(session.id)} />
                            </Tooltip>
                        </div>
                    </div>
                </div>
            ))}
            {!sessions.length ? (
                <div className="px-3 py-8 text-center text-sm" style={{ color: theme.node.muted }}>
                    {canvasT("videoCanvas.agent.historyEmptyHint", "网站 Agent 的对话记录会显示在这里")}
                </div>
            ) : null}
        </div>
    );
}

export function OnlineAgentSetupView({ theme, activeModel, onOpenConfig }: { theme: (typeof canvasThemes)[keyof typeof canvasThemes]; activeModel: string; onOpenConfig: () => void }) {
    return (
        <div className="thin-scrollbar min-h-0 flex-1 overflow-y-auto p-4">
            <div className="space-y-4">
                <div>
                    <div className="text-base font-semibold leading-6">{canvasT("videoCanvas.agent.setupTitle", "连接配置")}</div>
                    <div className="mt-1 text-xs leading-5" style={{ color: theme.node.muted }}>
                        {canvasT("videoCanvas.agent.setupHint", "网站 Agent 直接使用当前网页配置的文本模型和 API。")}
                    </div>
                </div>
                <div className="rounded-md p-3" style={{ background: theme.spatial.surface }}>
                    <div className="flex flex-wrap items-start justify-between gap-3">
                        <div className="min-w-0 flex-1">
                            <div className="text-sm font-medium leading-5">{canvasT("videoCanvas.agent.textModel", "文本模型")}</div>
                            <div className="mt-1 truncate text-xs leading-5" style={{ color: theme.node.muted }}>
                                {activeModel || canvasT("videoCanvas.agent.noModelConfigured", "未配置模型")}
                            </div>
                        </div>
                        <Button className="!h-8 !px-3" type="primary" icon={<Settings2 className="size-4" />} onClick={onOpenConfig}>
                            {canvasT("videoCanvas.agent.configure", "配置")}
                        </Button>
                    </div>
                </div>
            </div>
        </div>
    );
}

export function OnlineAgentLogView({ logs, theme, context, onClear }: { logs: OnlineAgentLog[]; theme: (typeof canvasThemes)[keyof typeof canvasThemes]; context: OnlineAgentLogContext; onClear: () => void }) {
    const [mode, setMode] = useState<"text" | "json">("text");
    const textareaRef = useRef<HTMLTextAreaElement>(null);
    const content = mode === "text" ? formatOnlineLogText(logs, context) : formatOnlineLogJson(logs, context);
    const lastError = [...logs].reverse().find((item) => /错误|失败|error|failed/i.test(`${item.title}\n${stringifyLog(item.data)}`));
    const copy = async (value = content) => {
        if (await copyToClipboard(value)) return;
        textareaRef.current?.focus();
        textareaRef.current?.select();
    };
    return (
        <div className="flex min-h-full flex-col gap-3">
            <div className="flex flex-wrap items-center justify-between gap-2">
                <Segmented size="small" value={mode} onChange={(value) => setMode(value as "text" | "json")} options={[{ label: canvasT("videoCanvas.agent.logDiagnose", "排查日志"), value: "text" }, { label: canvasT("videoCanvas.agent.logRawJson", "原始 JSON"), value: "json" }]} />
                <div className="flex items-center gap-2">
                    <span className="text-xs" style={{ color: theme.node.muted }}>{canvasT("videoCanvas.agent.logCount", "{{count}} 条", { count: logs.length })}</span>
                    <Button size="small" icon={<Copy className="size-3.5" />} disabled={!logs.length} onClick={() => void copy()}>{canvasT("videoCanvas.agent.copy", "复制")}</Button>
                    <Button size="small" disabled={!lastError} onClick={() => lastError && void copy(formatOnlineLogText([lastError], context))}>{canvasT("videoCanvas.agent.recentError", "最近错误")}</Button>
                    <Button size="small" danger type="text" icon={<Trash2 className="size-3.5" />} disabled={!logs.length} onClick={onClear}>{canvasT("videoCanvas.agent.clear", "清空")}</Button>
                </div>
            </div>
            <textarea
                ref={textareaRef}
                readOnly
                value={content}
                className="thin-scrollbar min-h-[360px] flex-1 resize-none rounded-md border-0 p-3 font-mono text-xs leading-5 outline-none"
                style={{ background: theme.spatial.surface, color: theme.node.text }}
                onFocus={(event) => event.currentTarget.select()}
            />
        </div>
    );
}

export function AssistantChatMessages({
    messages,
    theme,
    user,
    busy,
    nodeCount,
    onSelectPrompt,
    onRejectTool,
    onApproveTool,
}: {
    messages: CanvasAssistantMessage[];
    theme: (typeof canvasThemes)[keyof typeof canvasThemes];
    user: LocalUser | null;
    busy: boolean;
    nodeCount: number;
    onSelectPrompt: (text: string) => void;
    onRejectTool?: (id: string) => void;
    onApproveTool?: (id: string) => void;
}) {
    // 映射结果按 messages 身份缓存；chat 项引用稳定后，流式只重渲变化的那条。
    const chatMessagePairs = useMemo(() => messages.map((message) => ({ chat: assistantMessageToChatMessage(message), message })), [messages]);
    if (!chatMessagePairs.length) {
        return <AgentChatEmptyState theme={theme} nodeCount={nodeCount} onSelect={onSelectPrompt} />;
    }
    return (
        <>
            {chatMessagePairs.map(({ chat, message }) => (
                <div key={chat.id} className="space-y-2">
                    <AgentChatMessage item={chat} theme={theme} user={user} onRejectTool={onRejectTool} onApproveTool={onApproveTool} />
                    {message.references?.length ? <MessageReferences message={message} /> : null}
                </div>
            ))}
            {busy ? <AgentWorkingMessage theme={theme} /> : null}
        </>
    );
}

function MessageReferences({ message }: { message: CanvasAssistantMessage }) {
    return (
        <div className={`flex max-w-[88%] flex-wrap gap-2 ${message.role === "user" ? "ml-auto justify-end" : "ml-11 justify-start"}`}>
            {message.references?.map((item, index, references) => (
                <AssistantReferenceChip key={item.id} item={item} label={assistantImageReferenceLabel(references, index)} />
            ))}
        </div>
    );
}

export function AssistantReferenceChip({ item, label, onRemove }: { item: CanvasAssistantReference; label?: string; onRemove?: () => void }) {
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const text = (item.text || item.title).replace(/\s+/g, " ").trim().slice(0, 1) || "文";
    return (
        <div className="group/chip relative inline-flex h-8 max-w-[150px] shrink-0 items-center gap-1.5 rounded-lg text-sm" style={{ color: theme.node.text }}>
            {item.dataUrl ? (
                <span className="relative block size-8 shrink-0">
                    <img src={item.dataUrl} alt="" className="size-8 rounded-lg object-cover" />
                    {label ? <span className="absolute left-0.5 top-0.5 rounded bg-black/60 px-1 py-0.5 text-[var(--fs-micro)] font-medium leading-none text-white">{label}</span> : null}
                </span>
            ) : (
                <span className="grid size-8 place-items-center rounded-md text-sm font-medium" style={{ background: theme.spatial.surface }}>
                    {text}
                </span>
            )}
            {onRemove ? (
                <button
                    type="button"
                    className="absolute -right-1 -top-1 grid size-4 place-items-center rounded-full border opacity-0 shadow-sm transition group-hover/chip:opacity-100"
                    style={{ background: theme.toolbar.panel, borderColor: theme.node.stroke }}
                    onClick={onRemove}
                    aria-label={canvasT("videoCanvas.agent.removeRef", "移除引用")}
                >
                    <X className="size-3" />
                </button>
            ) : null}
        </div>
    );
}

export function buildAssistantReferences(nodes: CanvasNodeData[], selectedNodeIds: Set<string>) {
    const nodeById = new Map(nodes.map((node) => [node.id, node]));
    return Array.from(selectedNodeIds)
        .map((id) => nodeById.get(id))
        .filter((node): node is CanvasNodeData => Boolean(node))
        .map(nodeToReference)
        .filter((item): item is CanvasAssistantReference => Boolean(item));
}

function nodeToReference(node: CanvasNodeData): CanvasAssistantReference | null {
    if (node.type === CanvasNodeType.Image && node.metadata?.content) {
        return { id: node.id, type: node.type, title: node.title, dataUrl: node.metadata.content, storageKey: node.metadata.storageKey };
    }
    if (node.type === CanvasNodeType.Text && node.metadata?.content) {
        return { id: node.id, type: node.type, title: node.title, text: node.metadata.content };
    }
    if (node.type === CanvasNodeType.Skill && node.metadata?.skillSnapshot) {
        return { id: node.id, type: node.type, title: node.title, text: [node.metadata.skillSnapshot.name, node.metadata.skillSnapshot.template, node.metadata.skillSnapshot.outputContract].filter(Boolean).join("\n\n") };
    }
    return null;
}

export function assistantImageReferenceLabel(references: CanvasAssistantReference[], index: number) {
    if (!references[index]?.dataUrl) return undefined;
    const imageIndex = references.slice(0, index + 1).filter((item) => item.dataUrl).length - 1;
    return imageIndex >= 0 ? imageReferenceLabel(imageIndex) : undefined;
}

function assistantMessageToChatMessage(message: CanvasAssistantMessage): CanvasAgentChatMessage {
    return { id: message.id, role: message.role, title: message.title, text: message.text, meta: message.meta, detail: message.detail };
}

function formatSessionTime(value?: string) {
    return value ? new Date(value).toLocaleString() : "";
}

function sessionPreview(session: CanvasAssistantSession) {
    return session.messages.at(-1)?.text || canvasT("videoCanvas.agent.messageCount", "{{count}} 条消息", { count: session.messages.length });
}

function stringifyLog(value: unknown) {
    return typeof value === "string" ? value : JSON.stringify(value, null, 2);
}

function formatOnlineLogText(logs: OnlineAgentLog[], context: OnlineAgentLogContext) {
    const head = [
        "影策网站 Agent 诊断日志",
        `model: ${context.model || "none"}`,
        `running: ${context.running}`,
        `confirmTools: ${context.confirmTools}`,
        `messages: ${context.messages}`,
        `nodes: ${context.nodes}`,
        `connections: ${context.connections}`,
        `logs: ${logs.length}`,
    ].join("\n");
    const body = logs.map((log, index) => [`#${index + 1} ${log.time} ${log.title}`, log.data === undefined ? "" : stringifyLog(log.data)].filter(Boolean).join("\n")).join("\n\n---\n\n");
    return [head, body || "暂无事件日志"].join("\n\n");
}

function formatOnlineLogJson(logs: OnlineAgentLog[], context: OnlineAgentLogContext) {
    return JSON.stringify({ context, logs: logs.map(({ time, title, data }) => ({ time, title, data })) }, null, 2);
}
