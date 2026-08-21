import { Fragment, useState, type CSSProperties, type ReactNode } from "react";
import { Dropdown } from "antd";
import { AlignLeft, ArrowRight, Bot, Check, ChevronDown, ChevronUp, Clapperboard, FolderKanban, Images, MoreHorizontal, Palette, Pencil, Plus, Sparkles, Type, Upload, X } from "lucide-react";
import { useTranslation } from "react-i18next";

import { resolveCanvasStylePreset } from "@oc/components/canvas/canvas-style-picker-modal";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { canvasThemes } from "@oc/lib/canvas-theme";
import type { CanvasShortDramaProgress, CanvasShortDramaStepId } from "@oc/lib/canvas/canvas-short-drama";
import { useThemeStore } from "@oc/stores/use-theme-store";
import type { CanvasNodeData } from "@oc/types/canvas";

export function CanvasLinkedProjectEmptyState({ projectName, hasChapter, onAddFirstChapter, onOpenAssets, onAddText }: { projectName: string; hasChapter: boolean; onAddFirstChapter: () => void; onOpenAssets: () => void; onAddText: () => void }) {
    useTranslation();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    return (
        <div className="pointer-events-none absolute inset-0 z-20 grid place-items-center px-4 pb-16 pt-20">
            <div className="pointer-events-auto w-full max-w-[440px] rounded-lg border p-3 shadow-sm backdrop-blur" data-canvas-no-zoom style={{ background: theme.toolbar.panel, borderColor: theme.toolbar.border, color: theme.node.text }}>
                <div className="flex items-center gap-2.5"><span className="grid size-8 shrink-0 place-items-center rounded-md" style={{ background: theme.toolbar.itemHover, color: theme.node.muted }}><FolderKanban className="size-4" /></span><div className="min-w-0"><h2 className="truncate text-sm font-semibold">{projectName}</h2><p className="mt-0.5 text-[var(--fs-label)]" style={{ color: theme.node.muted }}>{canvasT("videoCanvas.empty.projectEmpty", "项目画布为空")}</p></div></div>
                <div className="mt-3 grid grid-cols-3 gap-1.5">
                    <button type="button" disabled={!hasChapter} onClick={onAddFirstChapter} className="flex h-9 min-w-0 items-center justify-center gap-1 rounded-md border px-2 text-[var(--fs-label)] font-medium disabled:opacity-35" style={{ borderColor: theme.node.stroke, background: theme.node.fill }}><Plus className="size-3.5 shrink-0" /><span className="truncate">{canvasT("videoCanvas.empty.addFirstChapter", "添加首章")}</span></button>
                    <button type="button" onClick={onOpenAssets} className="flex h-9 min-w-0 items-center justify-center gap-1 rounded-md border px-2 text-[var(--fs-label)] font-medium" style={{ borderColor: theme.node.stroke, background: theme.node.fill }}><Images className="size-3.5 shrink-0" /><span className="truncate">{canvasT("videoCanvas.empty.projectAssets", "项目资产")}</span></button>
                    <button type="button" onClick={onAddText} className="flex h-9 min-w-0 items-center justify-center gap-1 rounded-md border px-2 text-[var(--fs-label)] font-medium" style={{ borderColor: theme.node.stroke, background: theme.node.fill }}><Type className="size-3.5 shrink-0" /><span className="truncate">{canvasT("videoCanvas.empty.newText", "新建文本")}</span></button>
                </div>
            </div>
        </div>
    );
}

export function CanvasShortDramaEmptyState({ onCreatePipeline, onOpenAgent, onUpload, onAddText, onAddScript }: {
    onCreatePipeline: () => void;
    onOpenAgent: () => void;
    onUpload: () => void;
    onAddText: () => void;
    onAddScript: () => void;
}) {
    useTranslation();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const focusStyle = { "--tw-ring-color": theme.accent.primary } as CSSProperties;
    return (
        <div className="pointer-events-none absolute inset-0 z-20 grid place-items-center px-4 pb-20 pt-24">
            <div className="pointer-events-auto w-full max-w-[760px]" data-canvas-no-zoom>
                <div className="mb-4 text-center">
                    <h2 className="text-lg font-semibold">{canvasT("videoCanvas.empty.whereToStart", "从哪里开始？")}</h2>
                    <p className="mt-1 text-sm" style={{ color: theme.node.muted }}>{canvasT("videoCanvas.empty.whereToStartHint", "选择一条主路径，之后仍可随时切换。")}</p>
                </div>
                <div className="grid gap-3 md:grid-cols-2">
                    <PathCard
                        icon={<Clapperboard className="size-5" />}
                        title={canvasT("videoCanvas.empty.createYourself", "自己创作")}
                        description={canvasT("videoCanvas.empty.createYourselfDesc", "搭好短剧骨架，再逐镜头编辑和生成。")}
                        action={canvasT("videoCanvas.empty.createPipeline", "创建短剧流水线")}
                        accent={theme.accent.primary}
                        theme={theme}
                        focusStyle={focusStyle}
                        onClick={onCreatePipeline}
                    />
                    <PathCard
                        icon={<Bot className="size-5" />}
                        title={canvasT("videoCanvas.empty.handToAgent", "交给 Agent")}
                        description={canvasT("videoCanvas.empty.handToAgentDesc", "用一句话描述题材、角色和核心冲突。")}
                        action={canvasT("videoCanvas.empty.oneSentenceGenerate", "一句话生成影视项目")}
                        accent={theme.node.activeStroke}
                        theme={theme}
                        focusStyle={focusStyle}
                        onClick={onOpenAgent}
                    />
                </div>
                <div className="mt-3 flex justify-center">
                    <Dropdown
                        trigger={["click"]}
                        menu={{
                            items: [
                                { key: "upload", icon: <Upload className="size-4" />, label: canvasT("videoCanvas.chrome.importMedia", "导入素材"), onClick: onUpload },
                                { key: "text", icon: <Type className="size-4" />, label: canvasT("videoCanvas.empty.newText", "新建文本"), onClick: onAddText },
                                { key: "storyboard", icon: <Clapperboard className="size-4" />, label: canvasT("videoCanvas.empty.newBlankStoryboard", "新建空白分镜"), onClick: onAddScript },
                            ],
                        }}
                    >
                        <button type="button" className="inline-flex h-8 items-center gap-1.5 rounded-md px-2.5 text-xs font-medium outline-none transition hover:bg-black/5 focus-visible:ring-2 dark:hover:bg-white/10" style={{ color: theme.node.muted, ...focusStyle }}>
                            <MoreHorizontal className="size-4" />{canvasT("videoCanvas.empty.otherStarts", "其他起点")}<ChevronDown className="size-3" />
                        </button>
                    </Dropdown>
                </div>
            </div>
        </div>
    );
}

export function CanvasFreeformEmptyState({ onUpload, onAddText }: { onUpload: () => void; onAddText: () => void }) {
    useTranslation();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    return (
        <div className="pointer-events-none absolute inset-0 z-20 grid place-items-center px-4 pb-20 pt-24">
            <div className="pointer-events-auto w-full max-w-[440px] rounded-lg border p-4 shadow-sm backdrop-blur" data-canvas-no-zoom style={{ background: theme.toolbar.panel, borderColor: theme.toolbar.border, color: theme.node.text }}>
                <div className="text-center"><h2 className="text-base font-semibold">{canvasT("videoCanvas.empty.blankStart", "从空白画布开始")}</h2><p className="mt-1 text-xs" style={{ color: theme.node.muted }}>{canvasT("videoCanvas.empty.blankStartHint", "添加文本或导入已有素材。")}</p></div>
                <div className="mt-4 grid grid-cols-2 gap-2">
                    <button type="button" onClick={onAddText} className="inline-flex h-10 items-center justify-center gap-2 rounded-md border text-sm font-medium" style={{ borderColor: theme.node.stroke, background: theme.node.fill }}><Type className="size-4" />{canvasT("videoCanvas.empty.newText", "新建文本")}</button>
                    <button type="button" onClick={onUpload} className="inline-flex h-10 items-center justify-center gap-2 rounded-md border text-sm font-medium" style={{ borderColor: theme.node.stroke, background: theme.node.fill }}><Upload className="size-4" />{canvasT("videoCanvas.chrome.importMedia", "导入素材")}</button>
                </div>
            </div>
        </div>
    );
}

function PathCard({ icon, title, description, action, accent, theme, focusStyle, onClick }: {
    icon: ReactNode;
    title: string;
    description: string;
    action: string;
    accent: string;
    theme: (typeof canvasThemes)[keyof typeof canvasThemes];
    focusStyle: CSSProperties;
    onClick: () => void;
}) {
    return (
        <section className="flex min-h-[176px] flex-col rounded-lg border p-4 shadow-sm backdrop-blur" style={{ background: theme.toolbar.panel, borderColor: theme.toolbar.border, color: theme.node.text }}>
            <span className="grid size-9 place-items-center rounded-md" style={{ background: `${accent}16`, color: accent }}>{icon}</span>
            <div className="mt-3 text-base font-semibold">{title}</div>
            <p className="mt-1 min-h-10 text-sm leading-5" style={{ color: theme.node.muted }}>{description}</p>
            <button type="button" className="mt-auto inline-flex h-9 w-full items-center justify-between rounded-md border px-3 text-sm font-semibold outline-none transition hover:brightness-105 focus-visible:ring-2" style={{ background: theme.node.fill, borderColor: theme.node.stroke, color: theme.node.text, ...focusStyle }} onClick={onClick}>
                <span>{action}</span><ArrowRight className="size-4" />
            </button>
        </section>
    );
}

export function CanvasShortDramaGuide({ progress, collapsed, onToggle, onSkip, onStepClick }: {
    progress: CanvasShortDramaProgress;
    collapsed: boolean;
    onToggle: () => void;
    onSkip: () => void;
    onStepClick: (stepId: CanvasShortDramaStepId) => void;
}) {
    useTranslation();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    if (!progress.active || collapsed) return null;
    return (
        <div data-canvas-no-zoom className="absolute left-1/2 top-[var(--canvas-topbar-offset)] z-[var(--z-toolbar)] flex max-w-[calc(100%_-_24px)] -translate-x-1/2 items-center gap-1 rounded-lg border p-1 shadow-sm backdrop-blur" style={{ background: theme.toolbar.panel, borderColor: theme.toolbar.border, color: theme.node.text }}>
            <div className="hide-scrollbar flex min-w-0 flex-1 items-center overflow-x-auto">
                <div className="flex shrink-0 items-center px-[var(--space-1)]">
                    {progress.steps.map((step, index) => (
                        <Fragment key={step.id}>
                            {index ? (
                                <span
                                    aria-hidden
                                    className="shrink-0 rounded-full transition-colors duration-150 motion-reduce:transition-none"
                                    style={{ width: "var(--flow-step-track-width)", height: "var(--flow-step-track-height)", background: step.status === "pending" ? theme.node.stroke : theme.accent.primary }}
                                />
                            ) : null}
                            <button
                                type="button"
                                aria-current={step.status === "current" ? "step" : undefined}
                                className="flex h-8 shrink-0 items-center gap-[var(--flow-step-gap)] rounded-md px-[var(--space-1-half)] outline-none transition-colors motion-reduce:transition-none hover:bg-black/5 focus-visible:ring-1 focus-visible:ring-inset dark:hover:bg-white/10"
                                style={{ "--tw-ring-color": theme.accent.primary } as CSSProperties}
                                onClick={() => onStepClick(step.id)}
                            >
                                <span
                                    className="grid shrink-0 place-items-center rounded-full font-semibold transition-all duration-150 motion-reduce:transition-none"
                                    style={{
                                        width: step.status === "current" ? "var(--flow-step-node-current)" : "var(--flow-step-node)",
                                        height: step.status === "current" ? "var(--flow-step-node-current)" : "var(--flow-step-node)",
                                        background: step.status === "pending" ? "transparent" : theme.accent.primary,
                                        border: step.status === "pending" ? "var(--stroke-2) solid var(--cn-stroke)" : "none",
                                        color: step.status === "pending" ? theme.node.muted : "#fff",
                                        boxShadow: step.status === "current" ? "var(--flow-step-current-glow)" : undefined,
                                        fontSize: step.status === "current" ? "var(--fs-body)" : "var(--fs-caption)",
                                    }}
                                >
                                    {step.status === "completed" ? <Check className="size-3.5" /> : index + 1}
                                </span>
                                <span
                                    className="whitespace-nowrap text-[var(--fs-caption)] font-semibold transition-colors duration-150 motion-reduce:transition-none"
                                    style={{ color: step.status === "current" ? theme.accent.primary : step.status === "completed" ? theme.node.text : theme.node.muted }}
                                >
                                    {step.id === "style" ? canvasT("videoCanvas.shortDrama.stepStyle", "选择画风")
                                        : step.id === "story" ? canvasT("videoCanvas.shortDrama.stepStory", "输入故事")
                                            : step.id === "storyboard" ? canvasT("videoCanvas.shortDrama.stepStoryboard", "生成分镜")
                                                : step.id === "video" ? canvasT("videoCanvas.shortDrama.stepVideo", "生成视频")
                                                    : canvasT("videoCanvas.shortDrama.stepFinal", "合并成片")}
                                </span>
                            </button>
                        </Fragment>
                    ))}
                </div>
            </div>
            <span className="mx-1 h-4 w-px shrink-0" style={{ background: theme.toolbar.border }} />
            {!progress.completed ? <button type="button" className="inline-flex h-8 shrink-0 items-center gap-1 rounded-md px-2 text-[var(--fs-label)] outline-none transition hover:bg-black/5 focus-visible:ring-2 dark:hover:bg-white/10" style={{ color: theme.node.muted, "--tw-ring-color": theme.accent.primary } as CSSProperties} onClick={onSkip}><X className="size-3" />{canvasT("videoCanvas.empty.skipGuide", "跳过导引")}</button> : null}
            <button type="button" className="grid size-8 shrink-0 place-items-center rounded-md outline-none transition hover:bg-black/5 focus-visible:ring-2 dark:hover:bg-white/10" style={{ color: theme.node.muted, "--tw-ring-color": theme.accent.primary } as CSSProperties} onClick={onToggle} aria-label={canvasT("videoCanvas.empty.collapseGuide", "折叠短剧流程")}><ChevronUp className="size-3.5" /></button>
        </div>
    );
}

export function CanvasStylePlaceholderNodeContent({ onChoose }: { onChoose: () => void }) {
    return <CanvasStyleNodeContent onChoose={onChoose} />;
}

export function CanvasStyleNodeContent({ node, onChoose }: { node?: CanvasNodeData; onChoose: () => void }) {
    useTranslation();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const preset = resolveCanvasStylePreset(node?.metadata?.stylePresetId);
    const hasStyle = Boolean(node?.metadata?.content);
    const title = preset?.title
        || (node?.title || "").replace(/^(?:项目)?画风\s*[·：:]?\s*/, "").trim()
        || canvasT("videoCanvas.empty.unnamedStyle", "未命名画风");
    const description = preset?.description || node?.metadata?.workflowDescription || canvasT("videoCanvas.empty.styleLockedDesc", "已锁定项目级美术与影像基线，下游分镜与生成会引用此规范。");

    if (!hasStyle) {
        return (
            <div className="flex h-full w-full flex-col items-center justify-center px-6 text-center" style={{ color: theme.node.text }}>
                <span className="grid size-10 place-items-center rounded-md" style={{ background: `${theme.accent.primary}16`, color: theme.accent.primary }}><Palette className="size-5" /></span>
                <div className="mt-3 text-sm font-semibold">{canvasT("videoCanvas.empty.projectStyle", "项目画风")}</div>
                <div className="mt-1 text-xs" style={{ color: theme.node.muted }}>{canvasT("videoCanvas.empty.pendingSelect", "待选择")}</div>
                <button type="button" className="mt-4 inline-flex h-8 items-center gap-1.5 rounded-md border px-3 text-xs font-medium outline-none transition hover:brightness-105 focus-visible:ring-2" style={{ background: theme.toolbar.panel, borderColor: theme.node.stroke, "--tw-ring-color": theme.accent.primary } as CSSProperties} onMouseDown={(event) => event.stopPropagation()} onClick={(event) => { event.stopPropagation(); onChoose(); }}><Sparkles className="size-3.5" />{canvasT("videoCanvas.empty.chooseStyle", "选择画风")}</button>
            </div>
        );
    }

    return (
        <div className="flex h-full w-full flex-col overflow-hidden" style={{ color: theme.node.text }}>
            <CanvasStyleCover imageUrl={preset?.imageUrl} accent={theme.accent.primary} muted={theme.node.muted} stroke={theme.node.stroke} panel={theme.canvas.background} />
            <div className="flex min-h-0 flex-1 flex-col px-4 py-3">
                <div className="truncate text-sm font-semibold" title={title}>{title}</div>
                <p className="mt-1 line-clamp-3 text-[var(--fs-label)] leading-5" style={{ color: theme.node.muted }}>{description}</p>
                <button
                    type="button"
                    className="mt-auto inline-flex h-8 w-fit items-center gap-1.5 rounded-md border px-3 text-xs font-medium outline-none transition hover:brightness-105 focus-visible:ring-2"
                    style={{ background: theme.toolbar.panel, borderColor: theme.node.stroke, "--tw-ring-color": theme.accent.primary } as CSSProperties}
                    onMouseDown={(event) => event.stopPropagation()}
                    onClick={(event) => { event.stopPropagation(); onChoose(); }}
                >
                    <Palette className="size-3.5" />
                    {node?.metadata?.locked ? canvasT("videoCanvas.empty.viewStyle", "查看画风") : canvasT("videoCanvas.empty.changeStyle", "更换画风")}
                </button>
            </div>
        </div>
    );
}

function CanvasStyleCover({ imageUrl, accent, muted, stroke, panel }: { imageUrl?: string; accent: string; muted: string; stroke: string; panel: string }) {
    const [failed, setFailed] = useState(false);
    const showImage = Boolean(imageUrl) && !failed;
    return (
        <div className="relative h-28 shrink-0 overflow-hidden border-b" style={{ borderColor: stroke, background: `linear-gradient(135deg, ${accent}22, ${panel})` }}>
            {showImage ? (
                <img
                    src={imageUrl}
                    alt=""
                    className="absolute inset-0 h-full w-full object-cover"
                    draggable={false}
                    onError={() => setFailed(true)}
                />
            ) : (
                <div className="flex h-full w-full items-center justify-center" style={{ color: muted }}>
                    <Palette className="size-7 opacity-70" />
                </div>
            )}
        </div>
    );
}


export function CanvasStoryInputNodeContent({ node, onEdit }: { node: CanvasNodeData; onEdit: () => void }) {
    useTranslation();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const content = (node.metadata?.content || "").replace(/\s+/g, " ").trim();
    return (
        <div className="flex h-full w-full flex-col overflow-hidden p-4" style={{ color: theme.node.text }}>
            <div className="flex items-center justify-between gap-3">
                <div className="flex min-w-0 items-center gap-2"><span className="grid size-8 shrink-0 place-items-center rounded-md" style={{ background: theme.toolbar.itemHover, color: theme.node.muted }}><AlignLeft className="size-4" /></span><span className="truncate text-sm font-semibold">{canvasT("videoCanvas.empty.storyOutline", "故事梗概")}</span></div>
            </div>
            <div className="mt-4 min-h-0 flex-1 overflow-hidden border-t pt-3 text-xs leading-6" style={{ borderColor: theme.node.stroke, color: content ? theme.node.muted : theme.node.placeholder }}>{content || canvasT("videoCanvas.empty.storyPlaceholder", "写下题材、角色、冲突和结局方向…")}</div>
            <button type="button" className="mt-3 inline-flex h-8 w-fit items-center gap-1.5 rounded-md px-2 text-xs font-medium outline-none transition hover:bg-black/5 focus-visible:ring-2 dark:hover:bg-white/10" style={{ color: theme.node.text, "--tw-ring-color": theme.accent.primary } as CSSProperties} onMouseDown={(event) => event.stopPropagation()} onClick={(event) => { event.stopPropagation(); onEdit(); }}><Pencil className="size-3.5" />{canvasT("videoCanvas.empty.editStory", "编辑故事")}</button>
        </div>
    );
}
