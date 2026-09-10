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
    drama: ["剧情", "Drama"],
    action: ["动作", "Action"],
    product: ["产品", "Product"],
    travel: ["旅拍", "Travel"],
    "music-mv": ["MV", "MV"],
    advertising: ["广告", "Ads"],
    documentary: ["纪录", "Doc"],
    aesthetic: ["美学", "Look"],
    storyboard: ["分镜", "Board"],
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
};

export function CanvasLibrarySheet({ open, tab: tabProp, onClose, onApplyRecipe, onApplyPlaybook, onApplyGraph, onInstallHub }: CanvasLibrarySheetProps) {
    useTranslation();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const [tab, setTab] = useState<LibraryTab>(tabProp || "recipe");
    const [group, setGroup] = useState<RecipeGroup | "all">("all");
    const [media, setMedia] = useState<CraftMediaScope>("all");
    const [category, setCategory] = useState<string>("all");
    const [query, setQuery] = useState("");
    const [hubItems, setHubItems] = useState<VimaxCloudSkill[]>([]);
    const [hubError, setHubError] = useState("");
    const [hubLoading, setHubLoading] = useState(false);

    useEffect(() => {
        if (open) setTab(tabProp || "recipe");
        else {
            setQuery("");
            setGroup("all");
            setMedia("all");
            setCategory("all");
        }
    }, [open, tabProp]);

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
            subtitle={canvasT("videoCanvas.craft.subtitle", "手法改当前镜头，手册给 Agent 读，工作流展开节点图。")}
            onClose={onClose}
        >
            <div style={{ color: theme.node.text }}>
                <div className="mb-3 flex flex-wrap gap-1.5">
                    <ChoiceChip selected={tab === "recipe"} theme={theme} onClick={() => setTab("recipe")}>{canvasT("videoCanvas.craft.tabRecipe", "手法")}</ChoiceChip>
                    <ChoiceChip selected={tab === "playbook"} theme={theme} onClick={() => setTab("playbook")}>{canvasT("videoCanvas.craft.tabPlaybook", "手册")}</ChoiceChip>
                    <ChoiceChip selected={tab === "graph"} theme={theme} onClick={() => setTab("graph")}>{canvasT("videoCanvas.craft.tabGraph", "工作流")}</ChoiceChip>
                    <ChoiceChip selected={tab === "community"} theme={theme} onClick={() => setTab("community")}>{canvasT("videoCanvas.craft.tabCommunity", "社区")}</ChoiceChip>
                </div>
                <label className="mb-3 flex items-center gap-1.5 rounded-md px-2" style={{ background: theme.toolbar.itemHover }}>
                    <Search className="size-3.5 shrink-0" style={{ color: theme.node.muted }} />
                    <input className="canvas-sheet-input h-8 flex-1 border-0 bg-transparent px-0" value={query} placeholder={canvasT("videoCanvas.craft.search", "搜索手法、手册或工作流")} onChange={(event) => setQuery(event.target.value)} />
                </label>
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
