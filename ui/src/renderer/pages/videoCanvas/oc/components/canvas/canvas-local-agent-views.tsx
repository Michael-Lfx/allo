import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button, Input, Segmented, Tooltip } from "antd";
import copyToClipboard from "copy-to-clipboard";
import { Copy, FolderOpen, KeyRound, Link2, PlugZap, Plus, RefreshCw, Trash2 } from "lucide-react";

import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { canvasThemes } from "@oc/lib/canvas-theme";
import type { AgentEventLog, AgentThreadSummary } from "@oc/stores/canvas/use-canvas-agent-store";
import { toolName } from "./canvas-local-agent-events";

const LA = "videoCanvas.agent.local";

function getAgentConnectSteps() {
    return [
        { title: canvasT(`${LA}.stepInstallTitle`, "从仓库安装插件"), text: canvasT(`${LA}.stepInstallText`, "插件暂未上架公共目录，请按项目 README 添加仓库 marketplace；安装后新建 Codex 对话。") },
        { title: canvasT(`${LA}.stepOpenTitle`, "打开画布连接"), text: canvasT(`${LA}.stepOpenText`, "回到这里点击连接，网页会自动读取本机 Agent 配置。") },
        { title: canvasT(`${LA}.stepManualTitle`, "手动启动备用"), text: canvasT(`${LA}.stepManualText`, "如果自动发现失败，请按插件说明启动本地 Agent 后再回到这里连接。") },
    ];
}

type AgentLogContext = { endpoint: string; connected: boolean; enabled: boolean; activity: string; waiting: boolean; sending: boolean; messages: number; pendingTool?: string };

function formatLogText(logs: AgentEventLog[], context: AgentLogContext) {
    const connectionStatus = context.connected ? canvasT(`${LA}.diagOnline`, "在线") : context.enabled ? canvasT(`${LA}.diagConnecting`, "连接中") : canvasT(`${LA}.diagDisabled`, "未启用");
    const head = [
        canvasT(`${LA}.diagHeader`, "影策 Canvas Agent 诊断日志"),
        canvasT(`${LA}.diagCanvasAgent`, "Canvas Agent: {{endpoint}}", { endpoint: context.endpoint }),
        canvasT(`${LA}.diagConnection`, "连接: {{status}}", { status: connectionStatus }),
        canvasT(`${LA}.diagStatus`, "状态: {{activity}}", { activity: context.activity }),
        `waiting: ${context.waiting}`,
        `sending: ${context.sending}`,
        `messages: ${context.messages}`,
        `pendingTool: ${context.pendingTool ? toolName(context.pendingTool) : "none"}`,
        `logs: ${logs.length}`,
    ].join("\n");
    const body = logs.map((item, index) => {
        const detail = item.raw == null ? item.text : JSON.stringify(item.raw, null, 2);
        return [`#${index + 1} ${item.time} ${item.title}`, detail].filter(Boolean).join("\n");
    }).join("\n\n---\n\n");
    return [head, body || canvasT(`${LA}.diagNoEvents`, "暂无事件日志")].join("\n\n");
}

function formatLogJson(logs: AgentEventLog[], context: AgentLogContext) {
    return JSON.stringify({ context, logs: logs.map(({ time, title, text, raw }) => ({ time, title, text, raw })) }, null, 2);
}

function formatThreadTime(value?: number) {
    if (!value) return "";
    return new Date(value * 1000).toLocaleString();
}

export function AgentLogView({ logs, theme, context, onClear, onCopied, onCopyBlocked }: { logs: AgentEventLog[]; theme: (typeof canvasThemes)[keyof typeof canvasThemes]; context: AgentLogContext; onClear: () => void; onCopied: (text: string) => void; onCopyBlocked: (text: string) => void }) {
    useTranslation();
    const [mode, setMode] = useState<"text" | "json">("text");
    const textareaRef = useRef<HTMLTextAreaElement>(null);
    const content = mode === "text" ? formatLogText(logs, context) : formatLogJson(logs, context);
    const lastError = [...logs].reverse().find((item) => /错误|失败|error|fail/i.test(`${item.title}\n${item.text}`));
    const copy = async (value = content, tip = canvasT(`${LA}.logCopied`, "日志已复制")) => {
        if (await copyToClipboard(value)) {
            onCopied(tip);
            return;
        }
        textareaRef.current?.focus();
        textareaRef.current?.select();
        onCopyBlocked(canvasT(`${LA}.logCopyManual`, "已选中日志，请手动复制"));
    };
    return (
        <div className="thin-scrollbar min-h-0 flex-1 overflow-y-auto p-4">
            <div className="flex min-h-full flex-col gap-3">
                <div>
                    <div className="text-base font-semibold leading-6">{canvasT(`${LA}.logTitle`, "运行日志")}</div>
                </div>
                <div className="flex flex-wrap items-center justify-between gap-2">
                    <Segmented size="small" value={mode} onChange={(value) => setMode(value as "text" | "json")} options={[{ label: canvasT("videoCanvas.agent.logDiagnose", "排查日志"), value: "text" }, { label: canvasT("videoCanvas.agent.logRawJson", "原始 JSON"), value: "json" }]} />
                    <div className="flex items-center gap-2">
                        <span className="text-xs" style={{ color: theme.node.muted }}>{canvasT("videoCanvas.agent.logCount", "{{count}} 条", { count: logs.length })}</span>
                        <Button size="small" icon={<Copy className="size-3.5" />} onClick={() => void copy()}>{canvasT("videoCanvas.agent.copy", "复制")}</Button>
                        <Button size="small" disabled={!lastError} onClick={() => lastError && void copy(formatLogText([lastError], context), canvasT(`${LA}.recentErrorCopied`, "最近错误已复制"))}>{canvasT("videoCanvas.agent.recentError", "最近错误")}</Button>
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
        </div>
    );
}

export function AgentConnectView({ theme, url, token, enabled, connected, activity, connectError, onUrlChange, onTokenChange, onToggleEnabled }: { theme: (typeof canvasThemes)[keyof typeof canvasThemes]; url: string; token: string; enabled: boolean; connected: boolean; activity: string; connectError: string; onUrlChange: (value: string) => void; onTokenChange: (value: string) => void; onToggleEnabled: () => void }) {
    useTranslation();
    const statusText = connectError ? canvasT(`${LA}.statusFailed`, "连接失败") : connected ? activity : enabled ? canvasT(`${LA}.statusConnecting`, "连接中") : canvasT(`${LA}.statusDisconnected`, "未连接");
    const statusColor = connectError ? "#dc2626" : connected ? "#16a34a" : enabled ? "#d97706" : theme.node.muted;
    const steps = getAgentConnectSteps();
    return (
        <div className="thin-scrollbar min-h-0 flex-1 overflow-y-auto p-4">
            <div className="space-y-4">
                <div>
                    <div className="text-base font-semibold leading-6">{canvasT(`${LA}.setupTitle`, "连接本地 Agent")}</div>
                    <div className="mt-1 text-xs leading-5" style={{ color: theme.node.muted }}>
                        {canvasT(`${LA}.setupHint`, "安装仓库自带的 Codex 插件后，画布会优先自动连接本机 Agent。")}
                    </div>
                </div>
                <div className="space-y-2">
                    {steps.map((step) => (
                        <div key={step.title} className="rounded-lg px-3 py-2.5">
                            <div className="text-sm font-medium leading-5">{step.title}</div>
                            <div className="mt-1 text-xs leading-5" style={{ color: theme.node.muted }}>{step.text}</div>
                        </div>
                    ))}
                </div>
                <div className="rounded-md p-3" style={{ background: theme.spatial.surface }}>
                    <div className="flex flex-wrap items-start justify-between gap-3">
                        <div className="min-w-0 flex-1">
                            <div className="flex min-w-0 items-center gap-2">
                                <span className="shrink-0 text-sm font-medium leading-5">{canvasT(`${LA}.webConnect`, "网页连接")}</span>
                                <span className="inline-flex min-w-0 items-center gap-1.5 rounded-full px-2 py-0.5 text-[var(--fs-label)] leading-4" style={{ background: theme.node.fill, color: statusColor }}>
                                    <span className="size-1.5 shrink-0 rounded-full" style={{ background: statusColor }} />
                                    <span className="truncate">{statusText}</span>
                                </span>
                            </div>
                            <div className="mt-1 text-xs leading-5" style={{ color: theme.node.muted }}>
                                {canvasT(`${LA}.webConnectHint`, "默认自动读取 Local URL 和 Connect token，失败时再手动填写。")}
                            </div>
                        </div>
                        <Button className="!h-8 !px-3" type={enabled ? "default" : "primary"} icon={<PlugZap className="size-4" />} onClick={onToggleEnabled}>
                            {enabled ? canvasT(`${LA}.disconnect`, "断开") : canvasT(`${LA}.connect`, "连接")}
                        </Button>
                    </div>
                    <div className="mt-3 grid gap-2.5">
                        <label className="grid gap-1.5">
                            <span className="flex items-center gap-1.5 text-xs font-medium" style={{ color: theme.node.muted }}>
                                <Link2 className="size-3.5" />
                                {canvasT(`${LA}.localUrl`, "本地地址")}
                                <span className="font-normal opacity-70">Local URL</span>
                            </span>
                            <Input size="large" prefix={<Link2 className="mr-1 size-4" style={{ color: theme.node.faint }} />} value={url} onChange={(event) => onUrlChange(event.target.value)} placeholder={canvasT(`${LA}.urlPlaceholder`, "例如 http://127.0.0.1:17371")} />
                        </label>
                        <label className="grid gap-1.5">
                            <span className="flex items-center gap-1.5 text-xs font-medium" style={{ color: theme.node.muted }}>
                                <KeyRound className="size-3.5" />
                                {canvasT(`${LA}.connectToken`, "连接 Token")}
                                <span className="font-normal opacity-70">Connect token</span>
                            </span>
                            <Input.Password size="large" prefix={<KeyRound className="mr-1 size-4" style={{ color: theme.node.faint }} />} value={token} onChange={(event) => onTokenChange(event.target.value)} placeholder={canvasT(`${LA}.tokenPlaceholder`, "自动发现，或手动填入 Connect token")} />
                        </label>
                        {connectError ? (
                            <div className="rounded-md px-2.5 py-2 text-xs leading-5" style={{ background: "rgba(220,38,38,.08)", color: "#dc2626" }}>
                                {connectError}
                            </div>
                        ) : null}
                    </div>
                </div>
            </div>
        </div>
    );
}

export function AgentHistoryView({ theme, threads, activeThreadId, workspacePath, loading, connected, onRefresh, onNewThread, onResumeThread, onDeleteThread }: { theme: (typeof canvasThemes)[keyof typeof canvasThemes]; threads: AgentThreadSummary[]; activeThreadId: string; workspacePath: string; loading: boolean; connected: boolean; onRefresh: () => void; onNewThread: () => void; onResumeThread: (threadId: string) => void; onDeleteThread: (thread: AgentThreadSummary) => void }) {
    useTranslation();
    return (
        <div className="thin-scrollbar min-h-0 flex-1 overflow-y-auto p-3">
            <div className="space-y-3">
                <div className="flex min-w-0 items-center gap-2 text-xs" style={{ color: theme.node.muted }}>
                    <FolderOpen className="size-3.5 shrink-0" />
                    <span className="shrink-0">{canvasT(`${LA}.workspace`, "工作空间")}</span>
                    <span className="min-w-0 truncate" title={workspacePath}>{workspacePath || canvasT(`${LA}.defaultWorkspace`, "默认画布目录")}</span>
                </div>
                <div className="flex flex-wrap items-center justify-between gap-2">
                    <div className="text-sm" style={{ color: theme.node.muted }}>
                        {threads.length ? canvasT("videoCanvas.agent.historyCount", "{{count}} 条历史", { count: threads.length }) : connected ? canvasT("videoCanvas.agent.noHistory", "暂无历史") : canvasT(`${LA}.notConnected`, "未连接")}
                    </div>
                    <div className="flex items-center gap-2">
                        <Button size="small" icon={<RefreshCw className={`size-3.5 ${loading ? "animate-spin" : ""}`} />} disabled={!connected || loading} onClick={onRefresh}>
                            {canvasT(`${LA}.refresh`, "刷新")}
                        </Button>
                        <Button size="small" type="primary" icon={<Plus className="size-3.5" />} disabled={!connected || loading} onClick={onNewThread}>
                            {canvasT("videoCanvas.agent.newChat", "新对话")}
                        </Button>
                    </div>
                </div>
                <div className="space-y-2">
                    {threads.map((thread) => {
                        const active = thread.id === activeThreadId;
                        return (
                            <div key={thread.id} className="rounded-md px-2.5 py-2 transition-colors" style={{ background: active ? theme.accent.primarySoft : "transparent", color: theme.node.text }}>
                                <div className="flex items-center gap-2">
                                    <div className="min-w-0 flex-1">
                                        <div className="flex min-w-0 items-center gap-1.5">
                                            {active ? <span className="shrink-0 text-[var(--fs-tiny)] font-medium" style={{ color: theme.node.text }}>{canvasT("videoCanvas.agent.current", "当前")}</span> : null}
                                            <div className="truncate text-sm font-medium leading-5">{thread.name || thread.preview || canvasT(`${LA}.unnamedChat`, "未命名对话")}</div>
                                        </div>
                                        <div className="truncate text-[var(--fs-label)] leading-4 opacity-65">{thread.preview || thread.id}</div>
                                    </div>
                                    <div className="flex shrink-0 items-center gap-1">
                                        <span className="text-[var(--fs-tiny)] opacity-55">{formatThreadTime(thread.updatedAt || thread.createdAt)}</span>
                                        <Button size="small" className="!h-6 !px-2" disabled={loading} onClick={() => onResumeThread(thread.id)}>
                                            {canvasT("videoCanvas.agent.enter", "进入")}
                                        </Button>
                                        <Tooltip title={canvasT("videoCanvas.agent.deleteRecord", "删除记录")}>
                                            <Button size="small" danger type="text" className="!h-6 !w-6 !min-w-6" disabled={loading} icon={<Trash2 className="size-3.5" />} onClick={() => onDeleteThread(thread)} />
                                        </Tooltip>
                                    </div>
                                </div>
                            </div>
                        );
                    })}
                    {!threads.length ? (
                        <div className="px-3 py-8 text-center text-sm" style={{ color: theme.node.muted }}>
                            {connected ? canvasT(`${LA}.historyEmptyConnected`, "当前工作空间还没有对话记录") : canvasT(`${LA}.historyEmptyDisconnected`, "连接本地 Agent 后显示历史记录")}
                        </div>
                    ) : null}
                </div>
            </div>
        </div>
    );
}
