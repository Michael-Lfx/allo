import { useTranslation } from "react-i18next";
import { memo, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { Button } from "antd";
import { ArrowUp, CheckCircle2, CircleAlert, ImagePlus, LoaderCircle, UserRound, Wrench, X, XCircle } from "lucide-react";

import { CanvasChromeButton } from "@oc/components/canvas/canvas-overlay";
import { canvasOverlayStyle } from "@oc/lib/canvas/canvas-overlay";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { canvasThemes } from "@oc/lib/canvas-theme";
import type { CanvasAgentOperationImpact } from "@oc/lib/canvas/canvas-agent-ops";
import type { CanvasResourceReference } from "@oc/lib/canvas/canvas-resource-references";
import type { LocalUser } from "@oc/stores/use-user-store";
import { CanvasResourceMentionTextarea } from "./canvas-resource-mention-textarea";

export type CanvasAgentChatAttachment = { id: string; name: string; url: string };
export type CanvasAgentMode = "online" | "local";
export type CanvasAgentChatMessage = {
    id: string;
    role: "user" | "assistant" | "system" | "tool" | "error";
    title?: string;
    text: string;
    meta?: string;
    detail?: unknown;
    attachments?: CanvasAgentChatAttachment[];
};

const WORKING_TEXT_KEY = "videoCanvas.agent.working";
const WORKING_TEXT_DEFAULT = "正在推演...";

export const AgentChatMessage = memo(function AgentChatMessage({
    item, theme, user, onRejectTool, onApproveTool }: { item: CanvasAgentChatMessage; theme: (typeof canvasThemes)[keyof typeof canvasThemes]; user: LocalUser | null; onRejectTool?: (id: string) => void; onApproveTool?: (id: string) => void }) {
    useTranslation();
    const isUser = item.role === "user";
    const isSystem = item.role === "system";
    const color = item.role === "error" ? "#dc2626" : item.role === "tool" ? "#2563eb" : theme.node.text;
    if (isSystem) {
        return (
            <div className="flex justify-center text-xs">
                <div className="max-w-[88%] px-3 py-1.5 text-center" style={{ color: theme.node.muted }}>
                    {item.text}
                    {item.meta ? <span className="ml-2 opacity-60">{item.meta}</span> : null}
                </div>
            </div>
        );
    }
    if (item.role === "tool") {
        if (objectField(item.detail, "status") === "pending") return <AgentPendingToolCard summary={item.text} detail={item.detail} theme={theme} onReject={() => onRejectTool?.(item.id)} onApprove={() => onApproveTool?.(item.id)} />;
        return (
            <div className="flex items-start gap-2.5">
                <AgentAvatar theme={theme} />
                <AgentToolCard title={item.title || canvasT("videoCanvas.agent.toolCall", "工具调用")} text={item.text} detail={item.detail} theme={theme} />
            </div>
        );
    }
    return (
        <div className={`flex items-start gap-2.5 ${isUser ? "justify-end" : "justify-start"}`}>
            {!isUser ? <AgentAvatar theme={theme} /> : null}
            <div className={`min-w-0 max-w-[86%] text-sm leading-6 ${isUser ? "rounded-md px-3 py-2.5 text-right" : "text-left"}`} style={{ color, ...(isUser ? { background: theme.accent.primarySoft } : {}) }}>
                <div className="whitespace-pre-wrap break-words text-left">{item.text}</div>
                {item.attachments?.length ? <AgentMessageAttachments attachments={item.attachments} /> : null}
                {item.meta ? <div className="mt-1 text-[var(--fs-label)] opacity-45">{item.meta}</div> : null}
            </div>
            {isUser ? <AgentUserAvatar user={user} theme={theme} /> : null}
        </div>
    );
});

export function AgentPendingToolCard({ summary, detail, theme, onReject, onApprove }: { summary: string; detail?: unknown; theme: (typeof canvasThemes)[keyof typeof canvasThemes]; onReject?: () => void; onApprove?: () => void }) {
    useTranslation();
    const plan = agentPlanFromDetail(detail);
    return (
        <div className="flex items-start gap-2.5">
            <AgentAvatar theme={theme} />
            <div className="min-w-0 flex-1 rounded-md p-3.5" style={{ background: "rgba(217,119,6,.07)", color: theme.node.text }}>
                <div className="flex items-start gap-3">
                    <span className="mt-0.5 grid size-7 shrink-0 place-items-center rounded-md" style={{ color: "#d97706", background: "rgba(217,119,6,.1)" }}>
                        <CircleAlert className="size-4" />
                    </span>
                    <div className="min-w-0 flex-1">
                        <div className="flex flex-wrap items-center gap-2 text-sm font-semibold leading-5">
                            <span>{plan?.title || canvasT("videoCanvas.agent.confirmTool", "确认工具调用")}</span>
                            <span className="inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[var(--fs-label)] font-medium" style={{ color: "#d97706", background: "rgba(217,119,6,.1)" }}>{canvasT("videoCanvas.agent.waitingConfirm", "等待确认")}</span>
                            {plan?.spend ? <span className="inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[var(--fs-label)] font-medium" style={{ color: "#d97706", background: "rgba(217,119,6,.1)" }}>{canvasT("videoCanvas.agent.planSpendBadge", "含花费")}</span> : null}
                        </div>
                        <div className="mt-2 text-sm leading-6" style={{ color: theme.node.text }}>{summary}</div>
                    </div>
                </div>
                {plan?.stages.length ? (
                    <ol className="mt-3 space-y-1.5">
                        {plan.stages.map((stage, index) => (
                            <li key={`${stage.label}-${index}`} className="flex gap-2 text-xs leading-5" style={{ color: theme.node.muted }}>
                                <span className="mt-0.5 w-4 shrink-0 tabular-nums" style={{ color: theme.node.faint }}>{index + 1}.</span>
                                <span style={{ color: stage.spend ? "#d97706" : theme.node.text }}>{stage.label}</span>
                            </li>
                        ))}
                    </ol>
                ) : null}
                {plan?.models.length ? (
                    <div className="mt-2 text-xs leading-5" style={{ color: theme.node.muted }}>
                        {canvasT("videoCanvas.agent.planModels", "模型：{{models}}", { models: plan.models.join("、") })}
                    </div>
                ) : null}
                {plan?.operationCount ? (
                    <div className="mt-3 pt-1">
                        <div className="grid grid-cols-2 gap-2">
                            <ImpactMetric label={canvasT("videoCanvas.agent.metricOps", "操作")} value={plan.operationCount} theme={theme} />
                            <ImpactMetric label={canvasT("videoCanvas.agent.metricNodes", "涉及节点")} value={plan.affectedNodeCount} theme={theme} />
                            <ImpactMetric label={canvasT("videoCanvas.agent.metricDelete", "删除")} value={plan.destructiveCount} attention={plan.destructiveCount > 0} theme={theme} />
                            <ImpactMetric label={canvasT("videoCanvas.agent.metricGenerate", "生成")} value={plan.generationCount} attention={plan.generationCount > 0} theme={theme} />
                        </div>
                        {plan.items.length ? <div className="mt-3 space-y-1.5">{plan.items.map((item, index) => <div key={`${item}-${index}`} className="flex gap-2 text-xs leading-5" style={{ color: theme.node.muted }}><span className="mt-2 size-1 shrink-0 rounded-full bg-current" /><span>{item}</span></div>)}</div> : null}
                        {plan.warning ? <div className="mt-3 rounded-md bg-amber-500/[.08] px-2.5 py-2 text-xs leading-5 text-amber-700 dark:text-amber-300">{plan.warning}</div> : null}
                    </div>
                ) : null}
                {detail ? <details className="mt-3 pt-1"><summary className="cursor-pointer text-xs" style={{ color: theme.node.muted }}>{canvasT("videoCanvas.agent.techDetails", "技术详情")}</summary><AgentDetailBlock detail={detail} theme={theme} /></details> : null}
                {onReject || onApprove ? (
                    <div className="mt-4 grid grid-cols-2 gap-2">
                        <Button danger className="!h-9" icon={<XCircle className="size-4" />} onClick={() => onReject?.()}>
                            {canvasT("videoCanvas.agent.reject", "拒绝执行")}
                        </Button>
                        <Button className="!h-9" icon={<CheckCircle2 className="size-4" />} style={{ borderColor: "rgba(22,163,74,.42)", color: "#16a34a", background: "transparent" }} onClick={() => onApprove?.()}>
                            {canvasT("videoCanvas.agent.approve", "批准执行")}
                        </Button>
                    </div>
                ) : null}
            </div>
        </div>
    );
}

function ImpactMetric({ label, value, attention = false, theme }: { label: string; value: number; attention?: boolean; theme: (typeof canvasThemes)[keyof typeof canvasThemes] }) {
    return <div className="px-1 py-1"><div className="text-[var(--fs-tiny)]" style={{ color: theme.node.muted }}>{label}</div><div className="mt-0.5 text-sm font-semibold tabular-nums" style={{ color: attention ? "#d97706" : theme.node.text }}>{value}</div></div>;
}

function agentPlanFromDetail(detail: unknown) {
    const impact = objectField(detail, "impact");
    if (!impact || typeof impact !== "object") return null;
    const value = impact as Partial<CanvasAgentOperationImpact> & { title?: unknown; stages?: unknown; models?: unknown; spend?: unknown };
    return {
        title: typeof value.title === "string" ? value.title : "",
        stages: Array.isArray(value.stages)
            ? value.stages.flatMap((item) => {
                if (!item || typeof item !== "object") return [];
                const stage = item as { label?: unknown; spend?: unknown };
                return typeof stage.label === "string" ? [{ label: stage.label, spend: Boolean(stage.spend) }] : [];
            })
            : [],
        models: Array.isArray(value.models) ? value.models.filter((item): item is string => typeof item === "string" && Boolean(item.trim())) : [],
        spend: Boolean(value.spend),
        operationCount: Number(value.operationCount) || 0,
        affectedNodeCount: Number(value.affectedNodeCount) || 0,
        destructiveCount: Number(value.destructiveCount) || 0,
        generationCount: Number(value.generationCount) || 0,
        items: Array.isArray(value.items) ? value.items.filter((item): item is string => typeof item === "string") : [],
        warning: typeof value.warning === "string" ? value.warning : "",
    };
}

// 序列化结果按 detail 对象身份缓存：流式期间列表反复重渲时不再对大快照重复 stringify。
const agentDetailTextCache = new WeakMap<object, string>();

export function AgentToolCard({ title, text, detail, theme }: { title: string; text: string; detail?: unknown; theme: (typeof canvasThemes)[keyof typeof canvasThemes] }) {
    useTranslation();
    const [detailOpen, setDetailOpen] = useState(false);
    const state = toolCardState(title, text, detail);
    return (
        <details className="min-w-0 flex-1 rounded-md px-3 py-3 text-left" style={{ background: theme.spatial.surface, color: theme.node.text }} onToggle={(event) => setDetailOpen((event.target as HTMLDetailsElement).open)}>
            <summary className="cursor-pointer list-none">
                <div className="flex items-start gap-3">
                    <span className="mt-0.5 grid size-7 shrink-0 place-items-center rounded-md" style={{ color: state.color, background: state.softBg }}>
                        {state.icon}
                    </span>
                    <div className="min-w-0 flex-1">
                        <div className="flex flex-wrap items-center gap-2 text-sm font-semibold leading-5">
                            <span className="min-w-0 truncate">{title}</span>
                            <span className="inline-flex shrink-0 items-center gap-1 rounded-full px-2 py-0.5 text-[var(--fs-label)] font-medium" style={{ color: state.color, background: state.softBg }}>
                                {state.label}
                            </span>
                            {detail ? <span className="ml-auto text-xs font-normal" style={{ color: theme.node.muted }}>{canvasT("videoCanvas.agent.details", "详情")}</span> : null}
                        </div>
                        <div className="mt-2 text-sm leading-6" style={{ color: state.isError ? state.color : theme.node.muted }}>
                            {text}
                        </div>
                    </div>
                </div>
            </summary>
            {/* 折叠时不挂载 <pre>，避免对完整工具结果（可能含压缩快照）反复 JSON.stringify */}
            {detail && detailOpen ? <AgentDetailBlock detail={detail} theme={theme} /> : null}
        </details>
    );
}

export function AgentWorkingMessage({ theme, label }: { theme: (typeof canvasThemes)[keyof typeof canvasThemes]; label?: string | null }) {
    useTranslation();
    const workingText = label?.trim() || canvasT(WORKING_TEXT_KEY, WORKING_TEXT_DEFAULT);
    return (
        <div className="flex items-start gap-2.5">
            <AgentAvatar theme={theme} />
            <div className="min-w-0 max-w-[82%]">
                <div className="flex items-center gap-2 text-sm" style={{ color: theme.node.muted }} aria-label={workingText}>
                    <LoaderCircle className="size-3.5 shrink-0 animate-spin" />
                    <span>{workingText}</span>
                </div>
            </div>
        </div>
    );
}

export function AgentChatComposer({
    prompt,
    attachments = [],
    disabled,
    sending,
    placeholder,
    theme,
    onPromptChange,
    onSubmit,
    onAddFiles,
    onRemoveAttachment,
    left,
    mentionReferences = [],
}: {
    prompt: string;
    attachments?: CanvasAgentChatAttachment[];
    disabled?: boolean;
    sending?: boolean;
    placeholder: string;
    theme: (typeof canvasThemes)[keyof typeof canvasThemes];
    onPromptChange: (value: string) => void;
    onSubmit: () => void;
    onAddFiles?: (files: FileList | File[] | null) => void | Promise<void>;
    onRemoveAttachment?: (id: string) => void;
    left?: ReactNode;
    mentionReferences?: CanvasResourceReference[];
}) {
    useTranslation();
    const fileInputRef = useRef<HTMLInputElement>(null);
    const canSubmit = !disabled && !sending && Boolean(prompt.trim() || attachments.length);
    return (
        <div
            className="px-3 pb-3 pt-1"
            onWheelCapture={(event) => {
                const target = event.target;
                // Select/Dropdown 弹层挂在 body，但仍沿 React 树冒泡到本组件；
                // 捕获阶段 stopPropagation 会阻断弹层内部的滚轮滚动。
                if (target instanceof Element && target.closest(".ant-select-dropdown, .ant-dropdown, .ant-popover, .ant-picker-dropdown, [data-canvas-wheel-scroll]")) {
                    return;
                }
                event.stopPropagation();
            }}
        >
            <div className="canvas-overlay rounded-lg px-3 pb-2.5 pt-3" style={canvasOverlayStyle(theme, { color: theme.accent.primary })}>
                {attachments.length ? (
                    <div className="thin-scrollbar mb-2 flex gap-2 overflow-x-auto pb-1">
                        {attachments.map((item) => (
                            <div key={item.id} className="group relative size-14 shrink-0 overflow-hidden rounded-md" title={item.name}>
                                <img src={item.url} alt={item.name} className="size-full object-cover" />
                                {onRemoveAttachment ? (
                                    <button type="button" className="absolute right-1 top-1 grid size-5 place-items-center rounded-full border opacity-0 shadow-sm transition group-hover:opacity-100" style={{ background: theme.toolbar.panel, borderColor: theme.node.stroke, color: theme.node.text }} onClick={() => onRemoveAttachment(item.id)} aria-label={canvasT("videoCanvas.agent.removeImage", "移除图片")}>
                                        <X className="size-3" />
                                    </button>
                                ) : null}
                            </div>
                        ))}
                    </div>
                ) : null}
                {mentionReferences.length ? (
                    <CanvasResourceMentionTextarea
                        value={prompt}
                        references={mentionReferences}
                        onChange={onPromptChange}
                        onSubmit={onSubmit}
                        sendOnEnter
                        placeholder={placeholder}
                        className="thin-scrollbar max-h-40 min-h-[60px] w-full resize-none border-0 bg-transparent px-1 py-1 text-sm leading-5 outline-none placeholder:opacity-45"
                        style={{ color: theme.node.text }}
                        onPaste={(event) => {
                            if (!onAddFiles) return;
                            const images = Array.from(event.clipboardData.files).filter((file) => file.type.startsWith("image/"));
                            if (!images.length) return;
                            event.preventDefault();
                            void onAddFiles(images);
                        }}
                    />
                ) : (
                    <textarea
                        value={prompt}
                        onChange={(event) => onPromptChange(event.target.value)}
                        onPaste={(event) => {
                            if (!onAddFiles) return;
                            const images = Array.from(event.clipboardData.files).filter((file) => file.type.startsWith("image/"));
                            if (!images.length) return;
                            event.preventDefault();
                            void onAddFiles(images);
                        }}
                        onKeyDown={(event) => {
                            if (event.key !== "Enter" || event.shiftKey || event.ctrlKey || event.metaKey) return;
                            event.preventDefault();
                            void onSubmit();
                        }}
                        className="thin-scrollbar max-h-40 min-h-[60px] w-full resize-none border-0 bg-transparent px-1 py-1 text-sm leading-5 outline-none placeholder:opacity-45"
                        style={{ color: theme.node.text }}
                        placeholder={placeholder}
                    />
                )}
                <div className="mt-2 flex items-center justify-between gap-2">
                    <div className="flex min-w-0 items-center gap-1">
                        {onAddFiles ? (
                            <>
                                <input ref={fileInputRef} hidden type="file" accept="image/*" multiple onChange={(event) => {
                                    void onAddFiles(event.target.files);
                                    event.target.value = "";
                                }} />
                                <CanvasChromeButton
                                    className="is-icon"
                                    disabled={sending}
                                    style={{ color: theme.node.muted }}
                                    title={canvasT("videoCanvas.agent.uploadImage", "上传图片")}
                                    aria-label={canvasT("videoCanvas.agent.uploadImage", "上传图片")}
                                    onClick={() => fileInputRef.current?.click()}
                                >
                                    <ImagePlus className="size-3.5" />
                                </CanvasChromeButton>
                            </>
                        ) : null}
                        {left}
                    </div>
                    <button
                        type="button"
                        className="canvas-send-token"
                        disabled={!canSubmit}
                        style={{
                            background: canSubmit ? theme.node.activeStroke : theme.toolbar.itemHover,
                            color: canSubmit ? theme.canvas.background : theme.node.faint,
                        }}
                        onClick={() => void onSubmit()}
                        aria-label={canvasT("videoCanvas.agent.send", "发送")}
                    >
                        {sending ? <LoaderCircle className="size-3 animate-spin" /> : <ArrowUp className="size-3" />}
                    </button>
                </div>
            </div>
        </div>
    );
}

export function AgentPanelTabs<T extends string>({ value, items, theme, right, onChange }: { value: T; items: { value: T; label: string; icon?: ReactNode; count?: number }[]; theme: (typeof canvasThemes)[keyof typeof canvasThemes]; right?: ReactNode; onChange: (value: T) => void }) {
    useTranslation();
    return (
        <div className="shrink-0 px-3 pb-2">
            <div className="flex min-h-9 items-center justify-between gap-2 rounded-md p-1" style={{ background: theme.spatial.surface }}>
                <nav className="grid min-w-0 flex-1 grid-flow-col auto-cols-fr items-center gap-0.5 text-xs" role="tablist" aria-label={canvasT("videoCanvas.agent.panelAria", "Agent 面板")}>
                    {items.map((item) => (
                        <button key={item.value} type="button" role="tab" aria-selected={value === item.value} className={`inline-flex h-8 min-w-0 items-center justify-center gap-1 rounded-[var(--r-sm)] px-1.5 transition-colors ${value === item.value ? "font-medium" : "font-normal"}`} style={{ background: value === item.value ? theme.node.fill : "transparent", color: value === item.value ? theme.node.text : theme.node.muted, boxShadow: value === item.value ? `0 1px 5px ${theme.spatial.shadow}` : "none" }} onClick={() => onChange(item.value)}>
                            <span className="shrink-0">{item.icon}</span>
                            <span className="min-w-0 truncate">{item.label}</span>
                            {item.count ? <span className="shrink-0 tabular-nums opacity-60">{item.count}</span> : null}
                        </button>
                    ))}
                </nav>
                {right ? <div className="flex shrink-0 items-center gap-1">{right}</div> : null}
            </div>
        </div>
    );
}

function AgentDetailBlock({ detail, theme }: { detail: unknown; theme: (typeof canvasThemes)[keyof typeof canvasThemes] }) {
    const detailText = useMemo(() => {
        if (!detail || typeof detail !== "object") return JSON.stringify(detail, null, 2);
        const cached = agentDetailTextCache.get(detail);
        if (cached !== undefined) return cached;
        const text = JSON.stringify(detail, null, 2);
        agentDetailTextCache.set(detail, text);
        return text;
    }, [detail]);
    return (
        <pre className="thin-scrollbar mt-3 max-h-64 overflow-auto rounded-md p-3 text-[var(--fs-label)] leading-4" style={{ background: theme.toolbar.panel, color: theme.node.muted }}>
            {detailText}
        </pre>
    );
}

function AgentAvatar({ theme }: { theme: (typeof canvasThemes)[keyof typeof canvasThemes] }) {
    return (
        <span className="grid size-7 shrink-0 place-items-center" role="img" aria-label="OpenAI">
            <span className="size-4 opacity-80" style={{ background: theme.node.text, WebkitMask: "url(/icons/openai.svg) center / contain no-repeat", mask: "url(/icons/openai.svg) center / contain no-repeat" }} />
        </span>
    );
}

function AgentUserAvatar({ user, theme }: { user: LocalUser | null; theme: (typeof canvasThemes)[keyof typeof canvasThemes] }) {
    const avatarUrl = user?.avatarUrl?.trim();
    return (
        <span className="grid size-7 shrink-0 place-items-center overflow-hidden rounded-full" style={{ color: theme.node.text }}>
            {avatarUrl ? <img src={avatarUrl} alt="" className="size-full object-cover" referrerPolicy="no-referrer" /> : <UserRound className="size-4" />}
        </span>
    );
}

function AgentMessageAttachments({ attachments }: { attachments: CanvasAgentChatAttachment[] }) {
    return (
        <div className="mt-2 grid grid-cols-3 gap-1.5">
            {attachments.map((item) => (
                <img key={item.id} src={item.url} alt={item.name} className="aspect-square w-full rounded-lg object-cover" />
            ))}
        </div>
    );
}

function toolCardState(title: string, text: string, detail?: unknown) {
    const raw = `${title} ${text} ${normalizeText(objectField(detail, "error"))}`;
    const lower = raw.toLowerCase();
    const tool = String(objectField(detail, "name") || objectField(detail, "tool") || "");
    if (objectField(detail, "status") === "running") return { label: canvasT("videoCanvas.agent.statusRunning", "执行中"), color: "#2563eb", softBg: "rgba(37,99,235,.08)", icon: <LoaderCircle className="size-4 animate-spin" />, isError: false };
    if (objectField(detail, "status") === "noop" || /未生效|无需|没有找到|没有.*可|已存在/.test(raw)) return { label: canvasT("videoCanvas.agent.statusNoop", "未生效"), color: "#d97706", softBg: "rgba(217,119,6,.04)", icon: <CircleAlert className="size-4" />, isError: false };
    if (/拒绝|取消/.test(raw) || lower.includes("rejected")) return { label: canvasT("videoCanvas.agent.statusRejected", "拒绝执行"), color: "#dc2626", softBg: "rgba(220,38,38,.04)", icon: <XCircle className="size-4" />, isError: true };
    if (objectField(detail, "status") === "failed" || /失败|错误/.test(raw) || lower.includes("failed") || lower.includes("error")) return { label: canvasT("videoCanvas.agent.statusFailed", "执行失败"), color: "#dc2626", softBg: "rgba(220,38,38,.04)", icon: <XCircle className="size-4" />, isError: true };
    if (objectField(detail, "status") === "completed" || /完成|成功/.test(raw) || lower.includes("completed") || lower.includes("succeeded")) return { label: tool === "canvas_apply_ops" || /画布操作/.test(title) ? canvasT("videoCanvas.agent.statusApproved", "已批准执行") : canvasT("videoCanvas.agent.statusCompleted", "执行完成"), color: "#16a34a", softBg: "rgba(22,163,74,.04)", icon: <CheckCircle2 className="size-4" />, isError: false };
    return { label: canvasT("videoCanvas.agent.toolCall", "工具调用"), color: "#2563eb", softBg: "rgba(37,99,235,.04)", icon: <Wrench className="size-4" />, isError: false };
}

function normalizeText(value: unknown) {
    if (typeof value === "string") return value.trim();
    if (value instanceof Error) return value.message;
    if (value == null) return "";
    return JSON.stringify(value, null, 2);
}

function objectField(value: unknown, key: string) {
    return value && typeof value === "object" ? (value as Record<string, unknown>)[key] : undefined;
}
