import type { CraftPlaybook } from "./types";

function playbook(input: {
    id: string;
    zh: string;
    en: string;
    jobZh: string;
    jobEn: string;
    category: string;
    brief: string;
}): CraftPlaybook {
    return {
        kind: "playbook",
        id: input.id,
        qualifiedId: `builtin:${input.id}`,
        title: { zh: input.zh, en: input.en },
        job: { zh: input.jobZh, en: input.jobEn },
        category: input.category,
        coverLookId: input.id,
        brief: input.brief,
        origin: "builtin",
    };
}

const LOOP = `画布执行：canvas_get_skill 读本手册 → storyboard_inspect / storyboard_apply 改现有 Script 行 → spec_inspect 守规格 → canvas_apply 只作图结构兜底 → canvas_run 并等待。Look 只是视觉槽。队列未空时不要说完成。`;

export const BUILTIN_PLAYBOOKS: CraftPlaybook[] = [
    playbook({
        id: "short-drama",
        zh: "短剧导演",
        en: "Short drama",
        jobZh: "高密度短剧：钩子、欲望阻力、一记真反转",
        jobEn: "Dense drama: hook, want/obstacle, one reversal",
        category: "drama",

        brief: `短剧导演。钩子开场，欲望与阻力相撞，一次真反转，收束必须在画面上兑现。情绪外化，不写标签。\n${LOOP}`,
    }),
    playbook({
        id: "female-drama",
        zh: "女频情感",
        en: "Female-led drama",
        jobZh: "女频情感节奏与人物关系外化",
        jobEn: "Relationship rhythm made visible",
        category: "drama",

        brief: `女频情感。关系变化必须外化成站位、道具和视线，不要内心独白。\n${LOOP}`,
    }),
    playbook({
        id: "horror-suspense",
        zh: "悬疑惊悚",
        en: "Horror suspense",
        jobZh: "可读的悬疑，不靠全黑和血浆",
        jobEn: "Readable dread, not black frames or gore",
        category: "drama",

        brief: `悬疑。信息差可见，禁止全黑、血浆堆砌和廉价jump scare。\n${LOOP}`,
    }),
    playbook({
        id: "fight-fx",
        zh: "打斗动作",
        en: "Fight FX",
        jobZh: "打斗受力、距离、环境反馈",
        jobEn: "Impact, distance, environment feedback",
        category: "action",

        brief: `打斗。每击要有受力、距离变化和环境反馈，不要飘。\n${LOOP}`,
    }),
    playbook({
        id: "product-demo",
        zh: "产品演示",
        en: "Product demo",
        jobZh: "产品功能可见、手与材质优先",
        jobEn: "Hands and materials make the function visible",
        category: "product",

        brief: `产品演示。手与材质优先，功能必须在画面上被看见。\n${LOOP}`,
    }),
    playbook({
        id: "travel-master",
        zh: "旅拍地理",
        en: "Travel",
        jobZh: "旅拍地理证据，少航拍空镜",
        jobEn: "Geographic evidence, few empty aerials",
        category: "travel",

        brief: `旅拍。地理证据优先，少航拍空镜，人物与地点要有互动。\n${LOOP}`,
    }),
    playbook({
        id: "music-visual",
        zh: "音乐视觉",
        en: "Music visual",
        jobZh: "卡点、母题、歌词不堆字",
        jobEn: "Hits, motif, no lyric dumps",
        category: "music-mv",

        brief: `MV。卡点与视觉母题，不要把歌词堆在画面上。\n${LOOP}`,
    }),
    playbook({
        id: "luxury-tvc",
        zh: "高奢广告",
        en: "Luxury TVC",
        jobZh: "高奢材质与仪式感，禁网红滤镜",
        jobEn: "Material ritual, no influencer LUT",
        category: "advertising",

        brief: `高奢。材质、仪式、克制剪辑。禁止网红滤镜和夸张口播。\n${LOOP}`,
    }),
    playbook({
        id: "documentary-observational",
        zh: "观察纪录",
        en: "Observational doc",
        jobZh: "观察式，少解说堆砌",
        jobEn: "Observe, don't narrate over",
        category: "documentary",

        brief: `观察纪录。少解说，让动作和空间说话。\n${LOOP}`,
    }),
    playbook({
        id: "wes-anderson",
        zh: "对称色块",
        en: "Symmetry blocks",
        jobZh: "对称、色块、横向调度",
        jobEn: "Symmetry, blocks, lateral blocking",
        category: "aesthetic",

        brief: `对称色块调度：中轴构图、横向走位、有限色板。不要模仿商标或署名。\n${LOOP}`,
    }),
    playbook({
        id: "character-bible",
        zh: "角色圣经",
        en: "Character bible",
        jobZh: "先建角色资产再分镜，禁止中途换脸",
        jobEn: "Lock character assets before boarding",
        category: "storyboard",

        brief: `角色圣经。先设定图/三视图，再写分镜。中途禁止换脸换衣。\n${LOOP}`,
    }),
    playbook({
        id: "scene-bible",
        zh: "场景圣经",
        en: "Scene bible",
        jobZh: "先锁场景功能与时段",
        jobEn: "Lock place function and time of day",
        category: "storyboard",

        brief: `场景圣经。先锁空间功能与时段，再让人物进入。\n${LOOP}`,
    }),
    playbook({
        id: "script-to-board",
        zh: "剧本上板",
        en: "Script to board",
        jobZh: "只改现有 Script 行，不另起剧本节点",
        jobEn: "Patch existing Script rows only",
        category: "storyboard",

        brief: `剧本上板。只 storyboard_apply 现有行，禁止再造一个剧本节点。\n${LOOP}`,
    }),
    playbook({
        id: "replica-ref",
        zh: "参考复刻",
        en: "Replica",
        jobZh: "分析参考片的镜法再换内容",
        jobEn: "Steal craft from a reference, swap content",
        category: "storyboard",

        brief: `复刻。先分析参考片镜法、节奏、光，再换成用户内容。不要复制面孔。\n${LOOP}`,
    }),
    playbook({
        id: "multi-shot-cut",
        zh: "动态多切",
        en: "Multi-shot cut",
        jobZh: "动态多切，Montage 而不是一镜到底",
        jobEn: "Montage coverage, not one long take",
        category: "storyboard",

        brief: `多切。用覆盖和剪辑推进，不默认一镜到底。\n${LOOP}`,
    }),
    playbook({
        id: "tail-to-head",
        zh: "尾接首",
        en: "Tail to head",
        jobZh: "相邻镜强制尾帧=下一首帧",
        jobEn: "Last frame of n = first frame of n+1",
        category: "storyboard",

        brief: `尾接首。相邻镜头尾帧必须能接到下一镜首帧。\n${LOOP}`,
    }),
];

export const PLAYBOOK_CATEGORIES = [
    "drama",
    "action",
    "product",
    "travel",
    "music-mv",
    "advertising",
    "documentary",
    "aesthetic",
    "storyboard",
] as const;

export type PlaybookCategory = (typeof PLAYBOOK_CATEGORIES)[number];

export const PLAYBOOK_BY_ID = new Map(BUILTIN_PLAYBOOKS.map((item) => [item.id, item]));
export const PLAYBOOK_BY_QUALIFIED = new Map(BUILTIN_PLAYBOOKS.map((item) => [item.qualifiedId, item]));
