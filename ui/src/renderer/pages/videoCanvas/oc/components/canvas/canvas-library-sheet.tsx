import { Search } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { CanvasSheet } from "@oc/components/canvas/canvas-overlay";
import { CanvasStyleCoverSwatch } from "@oc/components/canvas/canvas-style-cover";
import { ChoiceChip } from "@oc/components/generation-settings-chrome";
import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { canvasThemes, type CanvasTheme } from "@oc/lib/canvas-theme";
import {
    BUILTIN_PLAYBOOKS,
    CRAFT_GRAPHS,
    CRAFT_RECIPES,
    PLAYBOOK_CATEGORIES,
    RECIPE_GROUPS,
    craftCover,
    craftText,
    recipeFitsMedia,
    recipeMediaKind,
    type CraftMediaKind,
    type CraftMediaScope,
    type CraftPlaybook,
    type CraftRecipe,
    type LibraryTab,
    type RecipeGroup,
} from "@oc/lib/canvas/craft/catalog";
import { listCanvasHubPlaybooks } from "@oc/lib/canvas/craft/hub";
import { fireGenerationTemplateEvent, getGenerationTemplateDetail, listGenerationTemplates } from "@oc/lib/canvas/generation-template/api";
import { templateInputKind, type GenerationTemplateDetail, type GenerationTemplateListItem, type TemplateInputKind } from "@oc/lib/canvas/generation-template/types";
import { formatCanvasUserError } from "@oc/lib/canvas/canvas-user-error";
import { useThemeStore } from "@oc/stores/use-theme-store";
import type { VimaxCloudSkill } from "@renderer/pages/videoGeneration/types";

export type { LibraryTab };

const GROUP_LABEL: Record<RecipeGroup, [string, string]> = {
    character: ["角色", "Character"],
    coverage: ["覆盖", "Coverage"],
    continuity: ["连续", "Continuity"],
    lighting: ["光影", "Light"],
    camera: ["运镜", "Camera"],
    craft: ["工艺", "Craft"],
};

const CATEGORY_LABEL: Record<string, [string, string]> = {
    cinematic: ["电影感", "Cinematic"],
    product: ["产品", "Product"],
    portrait: ["人像", "Portrait"],
    action: ["动作", "Action"],
    drama: ["剧情", "Drama"],
    aesthetic: ["美学", "Look"],
    advertising: ["广告", "Ads"],
    travel: ["旅拍", "Travel"],
    "music-mv": ["MV", "MV"],
    documentary: ["纪录", "Doc"],
    storyboard: ["分镜", "Board"],
};

const TEMPLATE_CATEGORIES = ["cinematic", "product", "portrait", "action", "drama", "aesthetic", "advertising", "travel", "music-mv"] as const;

const TEMPLATE_INPUT_LABEL: Record<TemplateInputKind, [string, string]> = {
    text: ["0 图", "0 image"],
    start: ["1 图首帧", "1 image start"],
    "start-end": ["2 图首尾", "2 image start/end"],
    reference: ["参考主体", "Subject refs"],
};

const INPUT_LABEL: Record<CraftRecipe["input"], [string, string]> = {
    text: ["文本", "Text"],
    "one-image": ["1 图", "1 image"],
    current: ["当前", "Current"],
    script: ["脚本", "Script"],
    "two-people": ["2 人", "2 people"],
};

const MEDIA_SCOPES: { id: CraftMediaScope; zh: string; key: string }[] = [
    { id: "all", zh: "全部", key: "videoCanvas.craft.mediaAll" },
    { id: "imageOnly", zh: "图片专用", key: "videoCanvas.craft.mediaImageOnly" },
    { id: "videoOnly", zh: "视频专用", key: "videoCanvas.craft.mediaVideoOnly" },
    { id: "both", zh: "图与视频", key: "videoCanvas.craft.mediaBoth" },
];

type CanvasLibrarySheetProps = {
    open: boolean;
    tab?: LibraryTab;
    onClose: () => void;
    onApplyRecipe: (recipe: CraftRecipe) => void;
    onApplyPlaybook: (playbook: CraftPlaybook) => void;
    onApplyGraph: (graphId: string) => void;
    onInstallHub: (skill: VimaxCloudSkill) => void;
    onApplyTemplate: (detail: GenerationTemplateDetail) => void | Promise<void>;
    canPublishFromCanvas?: boolean;
    onPublishFromCanvas?: () => void;
};

export function CanvasLibrarySheet({
    open,
    tab: tabProp,
    onClose,
    onApplyRecipe,
    onApplyPlaybook,
    onApplyGraph,
    onInstallHub,
    onApplyTemplate,
    canPublishFromCanvas,
    onPublishFromCanvas,
}: CanvasLibrarySheetProps) {
    useTranslation();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const [tab, setTab] = useState<LibraryTab>(tabProp || "template");
    const [group, setGroup] = useState<RecipeGroup | "all">("all");
    const [media, setMedia] = useState<CraftMediaScope>("all");
    const [category, setCategory] = useState<string>("all");
    const [query, setQuery] = useState("");
    const [templateOrigin, setTemplateOrigin] = useState<"all" | "official" | "ugc">("all");
    const [templateNodeType, setTemplateNodeType] = useState<"all" | "image" | "video">("all");
    const [templateCategory, setTemplateCategory] = useState<string>("all");
    const [hubItems, setHubItems] = useState<VimaxCloudSkill[]>([]);
    const [hubError, setHubError] = useState("");
    const [hubLoading, setHubLoading] = useState(false);
    const [templates, setTemplates] = useState<GenerationTemplateListItem[]>([]);
    const [templateError, setTemplateError] = useState("");
    const [templateLoading, setTemplateLoading] = useState(false);
    const [applyingTemplateId, setApplyingTemplateId] = useState<number | null>(null);

    useEffect(() => {
        if (open) setTab(tabProp || "template");
        else {
            setQuery("");
            setGroup("all");
            setMedia("all");
            setCategory("all");
            setTemplateOrigin("all");
            setTemplateNodeType("all");
            setTemplateCategory("all");
        }
    }, [open, tabProp]);

    useEffect(() => {
        if (!open || tab !== "template") return;
        let cancelled = false;
        setTemplateLoading(true);
        setTemplateError("");
        listGenerationTemplates({
            page: 1,
            pageSize: 48,
            keyword: query,
            origin: templateOrigin === "all" ? undefined : templateOrigin,
            nodeType: templateNodeType === "all" ? undefined : templateNodeType,
            category: templateCategory === "all" ? undefined : templateCategory,
        })
            .then((result) => {
                if (!cancelled) setTemplates(result.list || []);
            })
            .catch((error: unknown) => {
                if (!cancelled) setTemplateError(error instanceof Error ? error.message : canvasT("videoCanvas.craft.templateFailed", "模板加载失败"));
            })
            .finally(() => {
                if (!cancelled) setTemplateLoading(false);
            });
        return () => {
            cancelled = true;
        };
    }, [open, tab, query, templateOrigin, templateNodeType, templateCategory]);

    const templateImpressionKey = templates.map((item) => item.id).join(",");
    useEffect(() => {
        if (!open || tab !== "template" || templateLoading || !templateImpressionKey) return;
        for (const id of templateImpressionKey.split(",").map(Number).filter((value) => value > 0)) {
            fireGenerationTemplateEvent(id, "impression");
        }
    }, [open, tab, templateLoading, templateImpressionKey]);

    useEffect(() => {
        if (!open || tab !== "community") return;
        let cancelled = false;
        setHubLoading(true);
        setHubError("");
        listCanvasHubPlaybooks(query)
            .then((list) => {
                if (!cancelled) setHubItems(list);
            })
            .catch((error: unknown) => {
                if (!cancelled) setHubError(error instanceof Error ? error.message : canvasT("videoCanvas.craft.hubFailed", "社区手册加载失败"));
            })
            .finally(() => {
                if (!cancelled) setHubLoading(false);
            });
        return () => {
            cancelled = true;
        };
    }, [open, tab, query]);

    const recipes = useMemo(() => {
        const needle = query.trim().toLowerCase();
        return CRAFT_RECIPES.filter((item) => (group === "all" || item.group === group) && recipeFitsMedia(item, media) && (!needle || `${item.title.zh} ${item.title.en} ${item.job.zh} ${item.job.en}`.toLowerCase().includes(needle)));
    }, [group, media, query]);
    const playbooks = useMemo(() => {
        const needle = query.trim().toLowerCase();
        return BUILTIN_PLAYBOOKS.filter((item) => (category === "all" || item.category === category) && (!needle || `${item.title.zh} ${item.title.en} ${item.job.zh}`.toLowerCase().includes(needle)));
    }, [category, query]);
    const graphs = useMemo(() => {
        const needle = query.trim().toLowerCase();
        return CRAFT_GRAPHS.filter((item) => !needle || `${item.title.zh} ${item.title.en} ${item.job.zh}`.toLowerCase().includes(needle));
    }, [query]);

    const recipeSections = useMemo(() => {
        if (group !== "all" || query.trim()) return null;
        return RECIPE_GROUPS.map((id) => ({ id, items: recipes.filter((item) => item.group === id) })).filter((section) => section.items.length);
    }, [group, query, recipes]);
    const playbookSections = useMemo(() => {
        if (category !== "all" || query.trim()) return null;
        return PLAYBOOK_CATEGORIES.map((id) => ({ id, items: playbooks.filter((item) => item.category === id) })).filter((section) => section.items.length);
    }, [category, query, playbooks]);

    return (
        <CanvasSheet
            className="canvas-library-sheet"
            open={open}
            theme={theme}
            width="min(1080px, calc(100vw - 24px))"
            title={canvasT("videoCanvas.craft.title", "画布货架")}
            subtitle={canvasT("videoCanvas.craft.subtitle", "模板填镜头图，手法改当前提示词，手册给 Agent 读，工作流展开节点图。")}
            onClose={onClose}
        >
            <div style={{ color: theme.node.text }}>
                <div className="mb-3 flex flex-wrap gap-1.5">
                    <ChoiceChip selected={tab === "template"} theme={theme} onClick={() => setTab("template")}>{canvasT("videoCanvas.craft.tabTemplate", "模板")}</ChoiceChip>
                    <ChoiceChip selected={tab === "recipe"} theme={theme} onClick={() => setTab("recipe")}>{canvasT("videoCanvas.craft.tabRecipe", "手法")}</ChoiceChip>
                    <ChoiceChip selected={tab === "playbook"} theme={theme} onClick={() => setTab("playbook")}>{canvasT("videoCanvas.craft.tabPlaybook", "手册")}</ChoiceChip>
                    <ChoiceChip selected={tab === "graph"} theme={theme} onClick={() => setTab("graph")}>{canvasT("videoCanvas.craft.tabGraph", "工作流")}</ChoiceChip>
                    <ChoiceChip selected={tab === "community"} theme={theme} onClick={() => setTab("community")}>{canvasT("videoCanvas.craft.tabCommunity", "社区")}</ChoiceChip>
                </div>
                <label className="mb-3 flex items-center gap-1.5 rounded-md px-2" style={{ background: theme.toolbar.itemHover }}>
                    <Search className="size-3.5 shrink-0" style={{ color: theme.node.muted }} />
                    <input className="canvas-sheet-input h-8 flex-1 border-0 bg-transparent px-0" value={query} placeholder={canvasT("videoCanvas.craft.search", "搜索模板、手法、手册或工作流")} onChange={(event) => setQuery(event.target.value)} />
                </label>
                {tab === "template" ? (
                    <>
                        {canPublishFromCanvas && onPublishFromCanvas ? (
                            <button
                                type="button"
                                className="mb-3 flex w-full items-center justify-center rounded-md px-2 py-1.5 text-[var(--fs-tiny)] font-medium"
                                style={{ color: theme.accent.primary, background: theme.toolbar.itemHover }}
                                onClick={onPublishFromCanvas}
                            >
                                {canvasT("videoCanvas.craft.publishFromCanvas", "从当前镜头发布模板")}
                            </button>
                        ) : null}
                        <div className="mb-2 flex flex-wrap gap-1.5">
                            <ChoiceChip selected={templateOrigin === "all"} theme={theme} onClick={() => setTemplateOrigin("all")}>{canvasT("videoCanvas.craft.all", "全部")}</ChoiceChip>
                            <ChoiceChip selected={templateOrigin === "official"} theme={theme} onClick={() => setTemplateOrigin("official")}>{canvasT("videoCanvas.craft.official", "官方")}</ChoiceChip>
                            <ChoiceChip selected={templateOrigin === "ugc"} theme={theme} onClick={() => setTemplateOrigin("ugc")}>{canvasT("videoCanvas.craft.community", "社区")}</ChoiceChip>
                        </div>
                        <div className="mb-2 flex flex-wrap gap-1.5">
                            <ChoiceChip selected={templateCategory === "all"} theme={theme} onClick={() => setTemplateCategory("all")}>{canvasT("videoCanvas.craft.all", "全部")}</ChoiceChip>
                            {TEMPLATE_CATEGORIES.map((id) => (
                                <ChoiceChip key={id} selected={templateCategory === id} theme={theme} onClick={() => setTemplateCategory(id)}>{canvasT(`videoCanvas.craft.category.${id}`, CATEGORY_LABEL[id][0])}</ChoiceChip>
                            ))}
                        </div>
                        <div className="mb-3 flex flex-wrap gap-1.5">
                            <ChoiceChip selected={templateNodeType === "all"} theme={theme} onClick={() => setTemplateNodeType("all")}>{canvasT("videoCanvas.craft.mediaAll", "全部")}</ChoiceChip>
                            <ChoiceChip selected={templateNodeType === "image"} theme={theme} onClick={() => setTemplateNodeType("image")}>{canvasT("videoCanvas.craft.mediaImageOnly", "图片专用")}</ChoiceChip>
                            <ChoiceChip selected={templateNodeType === "video"} theme={theme} onClick={() => setTemplateNodeType("video")}>{canvasT("videoCanvas.craft.mediaVideoOnly", "视频专用")}</ChoiceChip>
                        </div>
                        {templateLoading ? (
                            <p className="py-10 text-center text-sm" style={{ color: theme.node.muted }}>{canvasT("videoCanvas.craft.templateLoading", "正在加载模板")}</p>
                        ) : templateError ? (
                            <p className="py-10 text-center text-sm" style={{ color: theme.node.muted }}>{templateError}</p>
                        ) : templates.length ? (
                            <CraftGrid>
                                {templates.map((item) => (
                                    <TemplateCard
                                        key={item.id}
                                        theme={theme}
                                        item={item}
                                        busy={applyingTemplateId === item.id}
                                        onClick={() => {
                                            setApplyingTemplateId(item.id);
                                            void getGenerationTemplateDetail(item.id)
                                                .then((detail) => onApplyTemplate(detail))
                                                .catch((error: unknown) => {
                                                    setTemplateError(formatCanvasUserError(error, canvasT("videoCanvas.craft.templateApplyFailed", "套用模板失败")));
                                                })
                                                .finally(() => setApplyingTemplateId(null));
                                        }}
                                    />
                                ))}
                            </CraftGrid>
                        ) : (
                            <p className="py-10 text-center text-sm" style={{ color: theme.node.muted }}>{canvasT("videoCanvas.craft.templateEmpty", "还没有可用的生成模板")}</p>
                        )}
                    </>
                ) : null}
                {tab === "recipe" ? (
                    <>
                        <div className="mb-2 flex flex-wrap gap-1.5">
                            <ChoiceChip selected={group === "all"} theme={theme} onClick={() => setGroup("all")}>{canvasT("videoCanvas.craft.all", "全部")}</ChoiceChip>
                            {RECIPE_GROUPS.map((id) => (
                                <ChoiceChip key={id} selected={group === id} theme={theme} onClick={() => setGroup(id)}>{canvasT(`videoCanvas.craft.group.${id}`, GROUP_LABEL[id][0])}</ChoiceChip>
                            ))}
                        </div>
                        <div className="mb-3 flex flex-wrap gap-1.5">
                            {MEDIA_SCOPES.map((item) => (
                                <ChoiceChip key={item.id} selected={media === item.id} theme={theme} onClick={() => setMedia(item.id)}>{canvasT(item.key, item.zh)}</ChoiceChip>
                            ))}
                        </div>
                        {recipes.length === 0 ? (
                            <EmptyFilter theme={theme} />
                        ) : recipeSections ? (
                            recipeSections.map((section) => (
                                <CraftSection key={section.id} theme={theme} title={canvasT(`videoCanvas.craft.group.${section.id}`, GROUP_LABEL[section.id][0])}>
                                    {section.items.map((item) => (
                                        <RecipeCard key={item.id} theme={theme} item={item} onClick={() => onApplyRecipe(item)} />
                                    ))}
                                </CraftSection>
                            ))
                        ) : (
                            <CraftGrid>
                                {recipes.map((item) => (
                                    <RecipeCard key={item.id} theme={theme} item={item} onClick={() => onApplyRecipe(item)} />
                                ))}
                            </CraftGrid>
                        )}
                    </>
                ) : null}
                {tab === "playbook" ? (
                    <>
                        <p className="mb-2 text-[var(--fs-tiny)] leading-4" style={{ color: theme.node.muted }}>{canvasT("videoCanvas.craft.playbookMediaHint", "手册管整条生产线，不分图片或视频专用。")}</p>
                        <div className="mb-3 flex flex-wrap gap-1.5">
                            <ChoiceChip selected={category === "all"} theme={theme} onClick={() => setCategory("all")}>{canvasT("videoCanvas.craft.all", "全部")}</ChoiceChip>
                            {PLAYBOOK_CATEGORIES.map((id) => (
                                <ChoiceChip key={id} selected={category === id} theme={theme} onClick={() => setCategory(id)}>{canvasT(`videoCanvas.craft.category.${id}`, CATEGORY_LABEL[id][0])}</ChoiceChip>
                            ))}
                        </div>
                        {playbooks.length === 0 ? (
                            <EmptyFilter theme={theme} />
                        ) : playbookSections ? (
                            playbookSections.map((section) => (
                                <CraftSection key={section.id} theme={theme} title={canvasT(`videoCanvas.craft.category.${section.id}`, CATEGORY_LABEL[section.id][0])}>
                                    {section.items.map((item) => (
                                        <CraftCard
                                            key={item.qualifiedId}
                                            theme={theme}
                                            cover={item.coverLookId}
                                            title={craftText(item.title)}
                                            job={craftText(item.job)}
                                            badge={canvasT(`videoCanvas.craft.category.${item.category}`, CATEGORY_LABEL[item.category]?.[0] || item.category)}
                                            onClick={() => onApplyPlaybook(item)}
                                        />
                                    ))}
                                </CraftSection>
                            ))
                        ) : (
                            <CraftGrid>
                                {playbooks.map((item) => (
                                    <CraftCard
                                        key={item.qualifiedId}
                                        theme={theme}
                                        cover={item.coverLookId}
                                        title={craftText(item.title)}
                                        job={craftText(item.job)}
                                        badge={canvasT(`videoCanvas.craft.category.${item.category}`, CATEGORY_LABEL[item.category]?.[0] || item.category)}
                                        onClick={() => onApplyPlaybook(item)}
                                    />
                                ))}
                            </CraftGrid>
                        )}
                    </>
                ) : null}
                {tab === "graph" ? (
                    graphs.length === 0 ? (
                        <EmptyFilter theme={theme} />
                    ) : (
                        <CraftGrid>
                            {graphs.map((item) => (
                                <CraftCard
                                    key={item.id}
                                    theme={theme}
                                    cover={item.coverLookId}
                                    title={craftText(item.title)}
                                    job={craftText(item.job)}
                                    badge={canvasT("videoCanvas.craft.kindGraph", "工作流")}
                                    onClick={() => onApplyGraph(item.id)}
                                />
                            ))}
                        </CraftGrid>
                    )
                ) : null}
                {tab === "community" ? (
                    hubLoading ? (
                        <p className="py-10 text-center text-sm" style={{ color: theme.node.muted }}>{canvasT("videoCanvas.craft.hubLoading", "正在加载社区手册")}</p>
                    ) : hubError ? (
                        <p className="py-10 text-center text-sm" style={{ color: theme.node.muted }}>{hubError}</p>
                    ) : hubItems.length ? (
                        <CraftGrid>
                            {hubItems.map((item) => (
                                <CraftCard
                                    key={item.id}
                                    theme={theme}
                                    cover="cinematic"
                                    imageUrl={item.coverUrl || item.previewUrl}
                                    title={item.displayName}
                                    job={item.description || item.displayName}
                                    badge={item.origin === "official" ? canvasT("videoCanvas.craft.official", "官方") : item.author?.name || canvasT("videoCanvas.craft.community", "社区")}
                                    onClick={() => onInstallHub(item)}
                                />
                            ))}
                        </CraftGrid>
                    ) : (
                        <p className="py-10 text-center text-sm" style={{ color: theme.node.muted }}>{canvasT("videoCanvas.craft.hubEmpty", "还没有兼容画布的社区手册")}</p>
                    )
                ) : null}
            </div>
        </CanvasSheet>
    );
}

function RecipeCard({ theme, item, onClick }: { theme: CanvasTheme; item: CraftRecipe; onClick: () => void }) {
    return (
        <CraftCard
            theme={theme}
            cover={item.coverLookId}
            title={craftText(item.title)}
            job={craftText(item.job)}
            badge={canvasT(`videoCanvas.craft.input.${item.input}`, INPUT_LABEL[item.input][0])}
            media={recipeMediaKind(item)}
            onClick={onClick}
        />
    );
}

function TemplateCard({
    theme,
    item,
    busy,
    onClick,
}: {
    theme: CanvasTheme;
    item: GenerationTemplateListItem;
    busy?: boolean;
    onClick: () => void;
}) {
    const [hover, setHover] = useState(false);
    const preview = item.previewUrl?.trim();
    const cover = item.coverUrl?.trim() || preview;
    const credits = item.estimatedCredits > 0 ? canvasT("videoCanvas.craft.templateCredits", "约 {{count}} 积分", { count: item.estimatedCredits }) : "";
    const inputKind = templateInputKind(item.target?.operations, item.assetRoles);
    const preferredModel = item.modelIntent?.preferred?.trim();
    const categoryLabel = item.category ? canvasT(`videoCanvas.craft.category.${item.category}`, CATEGORY_LABEL[item.category]?.[0] || item.category) : "";
    const nodeKind = (item.target?.nodeTypes || []).includes("video") && !(item.target?.nodeTypes || []).includes("image")
        ? "video"
        : (item.target?.nodeTypes || []).includes("image") && !(item.target?.nodeTypes || []).includes("video")
            ? "image"
            : undefined;
    return (
        <button
            type="button"
            disabled={busy}
            className="group flex w-full flex-col overflow-hidden rounded-2xl border text-left transition-[transform,box-shadow] duration-300 hover:-translate-y-0.5 disabled:opacity-60"
            style={{ background: theme.canvas.background, borderColor: theme.node.stroke, boxShadow: `0 10px 28px ${theme.spatial.shadow}` }}
            onClick={onClick}
            onMouseEnter={() => setHover(true)}
            onMouseLeave={() => setHover(false)}
        >
            <span className="relative aspect-video w-full overflow-hidden">
                {hover && preview ? (
                    <video src={preview} className="absolute inset-0 size-full object-cover" autoPlay loop muted playsInline />
                ) : cover ? (
                    <img src={cover} alt="" draggable={false} className="absolute inset-0 size-full object-cover transition duration-500 group-hover:scale-[1.06]" />
                ) : (
                    <CanvasStyleCoverSwatch cover={craftCover("cinematic")} className="absolute inset-0 size-full" hoverZoom />
                )}
                <span className="pointer-events-none absolute inset-0 bg-gradient-to-t from-black/70 via-black/10 to-transparent" />
                <span className="absolute left-2.5 top-2.5 rounded-full px-2 py-0.5 text-[10px] text-white/90" style={{ background: "rgba(0,0,0,.45)" }}>
                    {item.origin === "official" ? canvasT("videoCanvas.craft.official", "官方") : item.author?.name || canvasT("videoCanvas.craft.community", "社区")}
                </span>
                {nodeKind ? (
                    <span className="absolute right-2.5 top-2.5 rounded-full px-2 py-0.5 text-[10px] text-white/90" style={{ background: "rgba(0,0,0,.45)" }}>{mediaBadge(nodeKind)}</span>
                ) : null}
            </span>
            <span className="px-3 py-2.5">
                <span className="block truncate text-sm font-semibold">{item.title}</span>
                <span className="mt-1 line-clamp-2 block text-[var(--fs-tiny)] leading-4" style={{ color: theme.node.muted }}>{item.job}</span>
                <span className="mt-1.5 flex flex-wrap gap-1">
                    <span className="rounded-full px-1.5 py-0.5 text-[10px]" style={{ background: theme.toolbar.itemHover, color: theme.node.muted }}>
                        {canvasT(`videoCanvas.craft.templateInput.${inputKind}`, TEMPLATE_INPUT_LABEL[inputKind][0])}
                    </span>
                    {preferredModel ? (
                        <span className="rounded-full px-1.5 py-0.5 text-[10px]" style={{ background: theme.toolbar.itemHover, color: theme.node.muted }}>{preferredModel}</span>
                    ) : null}
                    {categoryLabel ? (
                        <span className="rounded-full px-1.5 py-0.5 text-[10px]" style={{ background: theme.toolbar.itemHover, color: theme.node.muted }}>{categoryLabel}</span>
                    ) : null}
                </span>
                {credits ? (
                    <span className="mt-1 block text-[10px]" style={{ color: theme.node.muted }}>{credits}</span>
                ) : null}
            </span>
        </button>
    );
}

function EmptyFilter({ theme }: { theme: CanvasTheme }) {
    return <p className="py-10 text-center text-sm" style={{ color: theme.node.muted }}>{canvasT("videoCanvas.craft.emptyFilter", "没有符合筛选的条目")}</p>;
}

function CraftSection({ theme, title, children }: { theme: CanvasTheme; title: string; children: React.ReactNode }) {
    return (
        <section className="mb-5 last:mb-0">
            <h3 className="mb-2 text-[11px] font-semibold tracking-wide" style={{ color: theme.node.muted }}>{title}</h3>
            <CraftGrid>{children}</CraftGrid>
        </section>
    );
}

function CraftGrid({ children }: { children: React.ReactNode }) {
    return <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-3">{children}</div>;
}

function mediaBadge(kind: CraftMediaKind): string {
    if (kind === "image") return canvasT("videoCanvas.craft.badgeImageOnly", "图专");
    if (kind === "video") return canvasT("videoCanvas.craft.badgeVideoOnly", "视专");
    return canvasT("videoCanvas.craft.badgeBoth", "图·视");
}

function CraftCard({
    theme,
    cover,
    imageUrl,
    title,
    job,
    badge,
    media,
    onClick,
}: {
    theme: CanvasTheme;
    cover: string;
    imageUrl?: string | null;
    title: string;
    job: string;
    badge: string;
    media?: CraftMediaKind;
    onClick: () => void;
}) {
    const swatch = craftCover(cover);
    if (imageUrl) swatch.image = imageUrl;
    return (
        <button type="button" className="group flex w-full flex-col overflow-hidden rounded-2xl border text-left transition-[transform,box-shadow] duration-300 hover:-translate-y-0.5" style={{ background: theme.canvas.background, borderColor: theme.node.stroke, boxShadow: `0 10px 28px ${theme.spatial.shadow}` }} onClick={onClick}>
            <CanvasStyleCoverSwatch cover={swatch} className="aspect-video w-full" hoverZoom alt={title}>
                <span className="pointer-events-none absolute inset-0 bg-gradient-to-t from-black/70 via-black/10 to-transparent" />
                <span className="absolute left-2.5 top-2.5 rounded-full px-2 py-0.5 text-[10px] text-white/90" style={{ background: "rgba(0,0,0,.45)" }}>{badge}</span>
                {media ? (
                    <span className="absolute right-2.5 top-2.5 rounded-full px-2 py-0.5 text-[10px] text-white/90" style={{ background: "rgba(0,0,0,.45)" }}>{mediaBadge(media)}</span>
                ) : null}
            </CanvasStyleCoverSwatch>
            <span className="px-3 py-2.5">
                <span className="block truncate text-sm font-semibold">{title}</span>
                <span className="mt-1 line-clamp-2 block text-[var(--fs-tiny)] leading-4" style={{ color: theme.node.muted }}>{job}</span>
            </span>
        </button>
    );
}
