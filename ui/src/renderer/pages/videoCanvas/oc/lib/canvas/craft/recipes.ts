import { CAMERA_MOVE_PRESETS } from "@oc/lib/canvas/cinematic-video-tools";
import type { CanvasGenerationMode } from "@oc/types/canvas";

import type { CraftRecipe, RecipeGroup, RecipeInput } from "./types";

const cameraById = new Map(CAMERA_MOVE_PRESETS.map((item) => [item.id, item]));

function recipe(input: {
    id: string;
    group: RecipeGroup;
    zh: string;
    en: string;
    jobZh: string;
    jobEn: string;
    prompt?: string;
    modes?: CanvasGenerationMode[];
    input?: RecipeInput;
    cameraMoveId?: string;
}): CraftRecipe {
    const camera = input.cameraMoveId ? cameraById.get(input.cameraMoveId) : undefined;
    const prompt = camera?.prompt || input.prompt;
    if (!prompt) throw new Error(`craft recipe ${input.id} missing prompt`);
    return {
        kind: "recipe",
        id: input.id,
        group: input.group,
        title: { zh: input.zh, en: input.en },
        job: { zh: input.jobZh, en: input.jobEn },
        prompt,
        modes: input.modes ?? ["image", "video"],
        input: input.input ?? "text",
        coverLookId: input.id,
        cameraMoveId: input.cameraMoveId,
    };
}

const character: CraftRecipe[] = [
    recipe({
        id: "character-sheet",
        group: "character",
        zh: "角色设定图",
        en: "Character sheet",
        jobZh: "同一人的正侧背+表情，供后续复用",
        jobEn: "Front/side/back plus expressions for reuse",
        input: "one-image",
        modes: ["image"],
        prompt: "生成角色设定图：保持同一角色身份、五官、发型、服装和体态一致，包含正面、侧面、背面和关键表情参考，背景简洁，便于后续镜头复用。",
    }),
    recipe({
        id: "turnaround-3view",
        group: "character",
        zh: "三视图",
        en: "Turnaround",
        jobZh: "三视图锁定体态与剪影",
        jobEn: "Lock silhouette with three views",
        input: "one-image",
        modes: ["image"],
        prompt: "生成同一角色的标准三视图：正面、侧面、背面并排，等比例、同一光照、同一服装与体态，背景纯净，剪影可读。",
    }),
    recipe({
        id: "expression-sheet",
        group: "character",
        zh: "表情设定",
        en: "Expression sheet",
        jobZh: "情绪不靠标签，用脸和手",
        jobEn: "Perform emotion with face and hands",
        input: "one-image",
        modes: ["image"],
        prompt: "同一角色的表情设定：用面部肌肉、眼神和手部动作外化情绪，不要写情绪标签。保持五官、发型、服装一致，网格或并排排列。",
    }),
    recipe({
        id: "wardrobe-sheet",
        group: "character",
        zh: "造型三套",
        en: "Wardrobe sheet",
        jobZh: "同一人三套造型，身份不漂",
        jobEn: "Three outfits, same identity",
        input: "one-image",
        modes: ["image"],
        prompt: "同一角色三套造型并排：日常、正装、行动装。五官、体态、年龄不变，只改服装与配饰，光线统一，背景简洁。",
    }),
    recipe({
        id: "group-blocking",
        group: "character",
        zh: "双人站位",
        en: "Group blocking",
        jobZh: "两人站位、视线、权力距离",
        jobEn: "Two-person blocking and eyelines",
        input: "two-people",
        prompt: "设计双人站位：明确谁靠近镜头、视线方向、身体朝向和权力距离。保持两人身份与服装，场景功能清楚，不要换成群像。",
    }),
    recipe({
        id: "prop-hero",
        group: "character",
        zh: "道具特写",
        en: "Hero prop",
        jobZh: "关键道具特写，可当贯穿母题",
        jobEn: "Hero prop close-up as motif",
        prompt: "关键道具英雄特写：材质、磨损、手与物的接触可见，光线塑形，背景简洁。道具要能在后续镜头里被认出来。",
    }),
    recipe({
        id: "location-plate",
        group: "character",
        zh: "场景建立",
        en: "Location plate",
        jobZh: "场景建立镜：功能、时段、生活痕迹",
        jobEn: "Establishing plate with time and traces",
        prompt: "场景建立镜：交代空间功能、时段和生活痕迹（家具、光线、使用过的物品），不要空旷展示厅。可容纳人物活动，但本镜以空间为主。",
    }),
    recipe({
        id: "identity-lock",
        group: "character",
        zh: "身份锁定",
        en: "Identity lock",
        jobZh: "重绘光影构图，禁止换脸换衣",
        jobEn: "Relight and reframe, never recast",
        input: "current",
        prompt: "基于当前画面重绘：只改构图、光影和镜头，禁止更换脸、发型、服装、年龄和场景身份。主体必须可被认成同一个人。",
    }),
];

const coverage: CraftRecipe[] = [
    recipe({
        id: "multi-angle",
        group: "coverage",
        zh: "多机位",
        en: "Coverage",
        jobZh: "远全中近特 + 侧背俯，可衔接",
        jobEn: "Wide to close, side/back/top, continuous",
        prompt: "围绕同一主体设计多机位画面，保持人物、服装、场景和光线一致，分别给出远景、全景、中景、近景、特写、侧面、背面和俯拍视角，镜头之间具有连续性。",
    }),
    recipe({
        id: "nine-grid",
        group: "coverage",
        zh: "九宫格",
        en: "Nine-grid",
        jobZh: "九宫格同一瞬间",
        jobEn: "Nine frames of the same instant",
        modes: ["image"],
        prompt: "把同一瞬间打成九宫格：同一角色、服装、场景和光线，变化机位、景别和表演，格与格可剪辑衔接。",
    }),
    recipe({
        id: "twenty-five-grid",
        group: "coverage",
        zh: "二十五宫格",
        en: "Twenty-five grid",
        jobZh: "25 宫格探索表演与机位",
        jobEn: "25-grid of performance and camera",
        modes: ["image"],
        prompt: "同一场景二十五宫格：锁定身份与空间，系统变化景别、机位高度、朝向和微表情，避免重复构图。",
    }),
    recipe({
        id: "ots-pair",
        group: "coverage",
        zh: "正反打",
        en: "OTS pair",
        jobZh: "正反打一对，轴线不穿",
        jobEn: "Over-the-shoulder pair, hold the line",
        input: "two-people",
        prompt: "正反打一对：过肩构图、轴线不穿、视线匹配。两人身份、服装、场景光线一致，景别接近。",
    }),
    recipe({
        id: "insert-cutaway",
        group: "coverage",
        zh: "插入切出",
        en: "Insert / cutaway",
        jobZh: "手、道具、环境插入镜",
        jobEn: "Hands, props, and environment inserts",
        prompt: "为当前场景写插入镜：手部、关键道具或环境细节，能剪进主镜头而不跳时空。保持光线方向与材质。",
    }),
    recipe({
        id: "establishing-to-close",
        group: "coverage",
        zh: "远落到脸",
        en: "Establish to close",
        jobZh: "从空间落到脸上的两拍",
        jobEn: "Two beats from space to face",
        prompt: "两拍覆盖：第一拍建立空间与人物位置，第二拍落到脸上的近景。身份、服装、时段连续，不要跳切换景。",
    }),
    recipe({
        id: "pov-shot",
        group: "coverage",
        zh: "主观镜头",
        en: "POV",
        jobZh: "角色视线里的世界",
        jobEn: "The world through the character's eyes",
        prompt: "主观镜头：画面是角色看见的世界，前景可有肩、手或眼镜边缘，焦点在被看对象。不要出现看者的正脸。",
    }),
    recipe({
        id: "split-dialogue",
        group: "coverage",
        zh: "对白双人",
        en: "Dialogue split",
        jobZh: "对白场景的双人构图方案",
        jobEn: "Two-shot plan for dialogue",
        input: "two-people",
        prompt: "对白双人构图：谁占画面、谁被前景遮挡、视线和嘴巴是否可见。保持轴线，给后续正反打留接口。",
    }),
];

const continuity: CraftRecipe[] = [
    recipe({
        id: "next-shot",
        group: "continuity",
        zh: "下一镜",
        en: "Next shot",
        jobZh: "下一镜：动作、视线、环境、运镜",
        jobEn: "Next shot: action, eyeline, space, camera",
        input: "current",
        prompt: "基于当前画面推演下一个连续镜头：保持角色和场景一致，明确主体接下来的动作、视线、环境变化、镜头运动和自然衔接方式，不要跳变构图或身份。",
    }),
    recipe({
        id: "prev-shot",
        group: "continuity",
        zh: "上一镜",
        en: "Previous shot",
        jobZh: "倒推上一拍，方便补覆盖",
        jobEn: "Infer the previous beat for coverage",
        input: "current",
        prompt: "倒推当前画面的上一拍：动作从哪来、视线从哪来、镜头从哪接上。身份与场景不变，只补能剪在前面的那一镜。",
    }),
    recipe({
        id: "story-beats",
        group: "continuity",
        zh: "连续节拍",
        en: "Story beats",
        jobZh: "短剧情拆成可生成节拍",
        jobEn: "Break a beat into shootable shots",
        modes: ["text", "image", "video"],
        input: "script",
        prompt: "把这段内容拆成连续镜头节拍。每个镜头写清主体动作、景别、构图、机位、运镜、光线、情绪和与前后镜头的衔接，并保持角色、场景和道具一致。",
    }),
    recipe({
        id: "first-last-bridge",
        group: "continuity",
        zh: "首尾桥",
        en: "First–last bridge",
        jobZh: "为首尾帧写可执行的中间运动",
        jobEn: "Write the motion between first and last frame",
        input: "current",
        modes: ["video"],
        prompt: "为首尾帧写中间运动：主体怎么走位、镜头如何移动、中间不要换身份或服装。动作必须能从第一帧到达最后一帧。",
    }),
    recipe({
        id: "match-cut",
        group: "continuity",
        zh: "图形匹配",
        en: "Match cut",
        jobZh: "图形匹配转场，形状/方向承接",
        jobEn: "Graphic match on shape or direction",
        prompt: "设计图形匹配转场：前后两镜共享形状、运动方向或色块，内容可变但剪辑点要对齐。写清匹配物是什么。",
    }),
    recipe({
        id: "action-bridge",
        group: "continuity",
        zh: "动作跨切",
        en: "Action bridge",
        jobZh: "动作跨切：出画入画匹配",
        jobEn: "Match on action in and out of frame",
        prompt: "动作跨切：前一镜出画动作与后一镜入画动作匹配，节奏连续。身份与道具方向一致，不要跳轴。",
    }),
];

const lighting: CraftRecipe[] = [
    recipe({
        id: "cinematic-light",
        group: "lighting",
        zh: "电影光影",
        en: "Cinematic light",
        jobZh: "保留内容，优化真实光线、层次和融合感",
        jobEn: "Keep content, improve light and falloff",
        input: "current",
        prompt: "保留主体身份、动作和原始构图，优化为真实电影摄影光线：明确主光方向、环境反射、阴影层次、肤色和背景融合，降低塑料感与过度锐化，不改变画面内容。",
    }),
    recipe({
        id: "golden-hour",
        group: "lighting",
        zh: "黄金时刻",
        en: "Golden hour",
        jobZh: "暖色侧光，长影，皮肤通透",
        jobEn: "Warm sidelight, long shadows, skin glow",
        input: "current",
        prompt: "改为黄金时刻光线：低角度暖侧光、长投影、空气感，皮肤与材质通透。不改变人物身份、服装和构图。",
    }),
    recipe({
        id: "neon-practical",
        group: "lighting",
        zh: "霓虹实光",
        en: "Neon practicals",
        jobZh: "招牌与室内灯作主光，禁止网红滤镜",
        jobEn: "Practical neon as key, no influencer LUT",
        input: "current",
        prompt: "用场景内霓虹、灯箱、屏幕作主光和轮廓光，颜色来自灯源而非滤镜。保留身份与空间，避免均匀紫青套滤。",
    }),
    recipe({
        id: "overcast-soft",
        group: "lighting",
        zh: "阴天柔光",
        en: "Overcast soft",
        jobZh: "阴天漫射，阴影干净",
        jobEn: "Soft overcast, clean shadows",
        input: "current",
        prompt: "改为阴天漫射光：天空作大柔光，阴影软、对比低、色彩克制。不改身份与场景，不要加阳光光斑。",
    }),
    recipe({
        id: "chiaroscuro",
        group: "lighting",
        zh: "明暗对照",
        en: "Chiaroscuro",
        jobZh: "一束主光，暗部有信息",
        jobEn: "One key light, readable darks",
        input: "current",
        prompt: "明暗对照：一束明确主光，暗部保留轮廓与材质，不要死黑。身份与构图不变，避免恐怖片血光。",
    }),
    recipe({
        id: "interior-practical",
        group: "lighting",
        zh: "室内实灯",
        en: "Interior practicals",
        jobZh: "台灯、窗、天花板灯分工",
        jobEn: "Lamps, window, and ceiling each do a job",
        input: "current",
        prompt: "室内实灯光：窗、台灯、顶灯分工明确，色温合理，桌面与脸有层次。不改房间布局和人物身份。",
    }),
];

const DUTCH_PROMPT = "镜头轻微荷兰角倾斜，保持主体清晰，强化不安或权力失衡，不改变场景、身份与服装。";

const camera: CraftRecipe[] = [
    recipe({ id: "cam-push-in", group: "camera", zh: "缓慢推近", en: "Push in", jobZh: "中景推到近景，压缩背景", jobEn: "Slow push from medium to close", cameraMoveId: "push_in", modes: ["video"], input: "current" }),
    recipe({ id: "cam-pull-out", group: "camera", zh: "缓慢后拉", en: "Pull out", jobZh: "近景拉出环境关系", jobEn: "Pull back to reveal space", cameraMoveId: "pull_out", modes: ["video"], input: "current" }),
    recipe({ id: "cam-orbit", group: "camera", zh: "环绕", en: "Orbit", jobZh: "半环绕主体，轮廓层次", jobEn: "Orbit to show contour", cameraMoveId: "orbit_left", modes: ["video"], input: "current" }),
    recipe({ id: "cam-pan-left", group: "camera", zh: "向左摇", en: "Pan left", jobZh: "水平向左展开信息", jobEn: "Pan left with the eyeline", cameraMoveId: "pan_left", modes: ["video"], input: "current" }),
    recipe({ id: "cam-pan-right", group: "camera", zh: "向右摇", en: "Pan right", jobZh: "水平向右带出环境", jobEn: "Pan right to reveal space", cameraMoveId: "pan_right", modes: ["video"], input: "current" }),
    recipe({ id: "cam-tilt-up", group: "camera", zh: "上仰", en: "Tilt up", jobZh: "由低向上，压迫或崇高", jobEn: "Tilt up for scale or power", cameraMoveId: "tilt_up", modes: ["video"], input: "current" }),
    recipe({ id: "cam-whip", group: "camera", zh: "甩镜", en: "Whip pan", jobZh: "快速横甩后稳定到新构图", jobEn: "Whip then settle on a new frame", cameraMoveId: "whip_pan", modes: ["video"], input: "current" }),
    recipe({ id: "cam-handheld", group: "camera", zh: "手持", en: "Handheld", jobZh: "微晃呼吸感，主体清晰", jobEn: "Breathing handheld, subject sharp", cameraMoveId: "handheld", modes: ["video"], input: "current" }),
    recipe({ id: "cam-crane", group: "camera", zh: "升起", en: "Crane up", jobZh: "从人物高度升起俯视", jobEn: "Rise from eye level to overhead", cameraMoveId: "crane_up", modes: ["video"], input: "current" }),
    recipe({ id: "cam-tracking", group: "camera", zh: "跟拍", en: "Tracking", jobZh: "正面后退跟拍，表情可见", jobEn: "Track backward, face readable", cameraMoveId: "tracking_forward", modes: ["video"], input: "current" }),
    recipe({ id: "cam-crash-zoom", group: "camera", zh: "急推变焦", en: "Crash zoom", jobZh: "光学变焦推进，机位不动", jobEn: "Optical zoom in, camera locked", cameraMoveId: "zoom_in", modes: ["video"], input: "current" }),
    recipe({
        id: "cam-dutch",
        group: "camera",
        zh: "荷兰角",
        en: "Dutch angle",
        jobZh: "轻微倾斜，不安或失衡",
        jobEn: "Slight Dutch tilt for unease",
        modes: ["image", "video"],
        input: "current",
        prompt: DUTCH_PROMPT,
    }),
];

const craft: CraftRecipe[] = [
    recipe({
        id: "video-prompt",
        group: "craft",
        zh: "时序提示",
        en: "Timed prompt",
        jobZh: "改写成时序化镜头指令",
        jobEn: "Rewrite as timed shot instructions",
        modes: ["text", "video"],
        input: "script",
        prompt: "将当前要求改写为结构化视频提示词，按时间顺序描述开场画面、主体动作、镜头运动、环境变化、声音和结束画面；消除冲突指令，保留所有关键约束。",
    }),
    recipe({
        id: "shot-language",
        group: "craft",
        zh: "镜头语言",
        en: "Shot language",
        jobZh: "强制补景别、机位、视线、调度",
        jobEn: "Force size, camera, eyeline, blocking",
        modes: ["text", "image", "video"],
        prompt: "把提示词补全为可执行镜头语言：景别、机位高度、视线、走位、前后景。不要只写气氛词。身份与场景约束全部保留。",
    }),
    recipe({
        id: "sound-pass",
        group: "craft",
        zh: "声音轨",
        en: "Sound pass",
        jobZh: "补对白/环境声/动作声",
        jobEn: "Add dialogue, room tone, foley",
        modes: ["text", "video", "audio"],
        prompt: "为当前镜头补声音：对白（若有）、环境声、动作声。声音必须能被画面解释，不要旁白堆砌。",
    }),
    recipe({
        id: "keep-content-restyle",
        group: "craft",
        zh: "只改气质",
        en: "Restyle, keep content",
        jobZh: "只改媒介气质，禁止改身份",
        jobEn: "Change medium, never identity",
        input: "current",
        prompt: "只改媒介气质、颗粒与色彩，禁止改脸、服装、道具和构图内容。结果必须仍是同一场戏。",
    }),
];

export const CRAFT_RECIPES: CraftRecipe[] = [...character, ...coverage, ...continuity, ...lighting, ...camera, ...craft];

export const RECIPE_GROUPS: RecipeGroup[] = ["character", "coverage", "continuity", "lighting", "camera", "craft"];

export const RECIPE_BY_ID = new Map(CRAFT_RECIPES.map((item) => [item.id, item]));
