import { projectStyleWorlds, type ProjectStyleWorldId } from "@oc/lib/canvas/canvas-style-system";
import { RECIPE_BY_ID } from "@oc/lib/canvas/craft/recipes";

export type CanvasGenerationJob = "none" | "character-sheet" | "turnaround" | "location-concept";

export type CanvasGenerationIntent = {
    job: CanvasGenerationJob;
    whiteBackground: boolean;
    styleWorldId?: ProjectStyleWorldId;
};

const STYLE_ALREADY = /【视觉风格】|【题材世界观】|【风格组合】/;
const JOB_ALREADY = /【作业】|生成角色设定图|生成同一角色的标准三视图|生成场景设定图/;
const WHITE_JOB_ALREADY = /纯白无缝白底/;

const STYLE_KEYWORDS: Array<{ world: ProjectStyleWorldId; pattern: RegExp }> = [
    { world: "wasteland", pattern: /废土|末日废墟|末日风|post[-\s]?apocalyptic|wasteland/i },
    { world: "cyberpunk", pattern: /赛博朋克|cyberpunk/i },
    { world: "xianxia", pattern: /仙侠|修仙/ },
    { world: "science-fiction", pattern: /科幻|sci[-\s]?fi/i },
    { world: "space", pattern: /星际|太空歌剧|space\s*opera/i },
    { world: "court", pattern: /宫廷|朝堂/ },
    { world: "campus", pattern: /校园/ },
    { world: "suspense", pattern: /悬疑|犯罪夜景/ },
    { world: "historical", pattern: /古装|历史剧/ },
    { world: "republic", pattern: /民国|年代胶片/ },
    { world: "pastoral", pattern: /田园|乡野/ },
    { world: "urban", pattern: /都市|职场/ },
];

const LOCATION_HINT = /超市|商场|店铺|门面|场景|地点|室内|建筑|街道|车站|医院|学校|仓库|工厂|location|environment|interior|store|market|building|street/i;
const TURNAROUND_HINT = /三视图|turnaround/i;
const CHARACTER_SHEET_HINT = /角色设定|人物设定|character\s*sheet/i;
const CONCEPT_HINT = /设定图|概念图|concept\s*(art|sheet|design)/i;
const WHITE_HINT = /纯白背景|白底|白色背景|seamless white|white background/i;

const LOCATION_CONCEPT_JOB = "【作业】场景设定图：把主体做成可复用的概念设定，而不是生活纪实照片。交代结构、材质、比例、招牌与入口；同一套设计语言；主体完整入画，边缘干净。";
const WHITE_BG_JOB = "背景为纯白无缝白底，无地面杂物、无环境故事、无干扰主体轮廓的投影。";

const IMAGE_VARIATIONS = [
    "",
    "【探索变体】换一个三分之四侧视角，主体完整入画，设计语言不变。",
    "【探索变体】换一个不同时段的光线（晨或暮），保持主体与设计不变。",
    "【探索变体】换一个更近的构图，或强调入口/局部细节，身份与设定不变。",
];

const CONCEPT_VARIATIONS = [
    "",
    "【探索变体】同一主体的四分之三侧设定，背景与比例保持一致。",
    "【探索变体】同一主体的背面或俯视结构设定，材质与比例不变。",
    "【探索变体】同一主体的材质、入口或局部细节特写设定，身份不变。",
];

const VIDEO_VARIATIONS = [
    "",
    "【探索变体】换一条略不同的运镜或机位，主体与场景身份不变。",
    "【探索变体】换一个不同时段的光线，动作与空间功能不变。",
    "【探索变体】略微改变起幅构图，保持同一场戏。",
];

export function detectCanvasGenerationIntent(prompt: string): CanvasGenerationIntent {
    const whiteBackground = WHITE_HINT.test(prompt);
    const styleWorldId = STYLE_KEYWORDS.find((item) => item.pattern.test(prompt))?.world;
    if (TURNAROUND_HINT.test(prompt)) return { job: "turnaround", whiteBackground, styleWorldId };
    if (CHARACTER_SHEET_HINT.test(prompt)) return { job: "character-sheet", whiteBackground, styleWorldId };
    if (CONCEPT_HINT.test(prompt)) {
        return { job: LOCATION_HINT.test(prompt) ? "location-concept" : "character-sheet", whiteBackground, styleWorldId };
    }
    return { job: "none", whiteBackground, styleWorldId };
}

export function enhanceCanvasGenerationPrompt(prompt: string, mode: "image" | "video" | "text" | "audio" = "image") {
    const trimmed = prompt.trim();
    const intent = detectCanvasGenerationIntent(trimmed);
    if (mode !== "image" && mode !== "video") return { prompt: trimmed, intent };
    const parts = [trimmed];
    if (mode === "image") {
        const jobPrompt = jobTemplate(intent.job);
        if (jobPrompt && !JOB_ALREADY.test(trimmed)) parts.push(jobPrompt);
        if (intent.whiteBackground && !WHITE_JOB_ALREADY.test(parts.join("\n"))) parts.push(WHITE_BG_JOB);
    }
    if (intent.styleWorldId && !STYLE_ALREADY.test(trimmed)) {
        const stylePrompt = compactWorldStyle(intent.styleWorldId);
        if (stylePrompt) parts.push(stylePrompt);
    }
    return { prompt: parts.filter(Boolean).join("\n"), intent };
}

export function isCanvasPromptOptimizeEnabled(value?: string) {
    return value !== "false";
}

export function polishVideoPrompt(prompt: string) {
    const trimmed = prompt.trim();
    if (!trimmed || /【镜头设计】/.test(trimmed) || trimmed.length >= 240) return trimmed;
    return `${trimmed}\n【镜头设计】按可拍摄镜头补全：明确主体动作、景别、运镜、光线和空间关系；不要改写用户已指定的内容、角色或风格。`;
}

export function varyCanvasGenerationPrompt(prompt: string, index: number, count: number, intent: CanvasGenerationIntent, mode: "image" | "video" = "image") {
    if (count <= 1 || index <= 0) return prompt;
    const table = mode === "video" ? VIDEO_VARIATIONS : intent.job === "none" ? IMAGE_VARIATIONS : CONCEPT_VARIATIONS;
    const variation = table[index % table.length];
    return variation ? `${prompt}\n${variation}` : prompt;
}

export function canvasGenerationSeed(batchIndex: number, salt: number) {
    const mixed = (Math.floor(Math.abs(salt)) ^ Math.imul(batchIndex + 1, 2654435761)) >>> 0;
    return mixed % 2147483647 || 1;
}

function jobTemplate(job: CanvasGenerationJob) {
    if (job === "character-sheet") return RECIPE_BY_ID.get("character-sheet")?.prompt;
    if (job === "turnaround") return RECIPE_BY_ID.get("turnaround-3view")?.prompt;
    if (job === "location-concept") return LOCATION_CONCEPT_JOB;
    return "";
}

function compactWorldStyle(worldId: ProjectStyleWorldId) {
    const world = projectStyleWorlds.find((item) => item.id === worldId);
    if (!world) return "";
    return `【视觉风格】${world.label}：${world.prompt} 色彩倾向：${world.palette}。服饰与场景：${world.wardrobe}；${world.environment}。避免：${world.forbidden}。`;
}
