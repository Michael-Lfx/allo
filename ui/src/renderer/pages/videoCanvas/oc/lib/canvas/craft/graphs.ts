import type { CraftGraph } from "./types";

function graph(id: string, zh: string, en: string, jobZh: string, jobEn: string): CraftGraph {
    return { kind: "graph", id, title: { zh, en }, job: { zh: jobZh, en: jobEn }, coverLookId: id };
}

export const CRAFT_GRAPHS: CraftGraph[] = [
    graph("starter-character", "角色资产链", "Character chain", "文本 → 设定图 → 三视图 → 表情", "Text → sheet → turnaround → expressions"),
    graph("starter-script-film", "剧本成片", "Script film", "脚本 → 静帧 → 视频", "Script → still → video"),
    graph("starter-product-ad", "产品广告", "Product ad", "产品图 + 角色 → 场景 → 成片", "Product + talent → scene → film"),
    graph("starter-explore-3", "三路对比", "Explore three", "同一提示分三路对比", "Same prompt, three looks"),
    graph("starter-replica", "参考复刻", "Replica", "参考视频 → 镜法分析 → 新片", "Reference → craft notes → new film"),
    graph("starter-mv", "音乐视觉", "Music visual", "音频 → 节拍 → 关键帧 → 镜头", "Audio → beats → keys → shots"),
    graph("starter-docu", "观察纪录", "Observational", "素材组 → 观察镜 → 字幕", "Assets → observe → captions"),
    graph("starter-poster-motion", "海报微动", "Poster motion", "海报静帧 → 微动视频", "Poster still → subtle motion"),
];

export const GRAPH_BY_ID = new Map(CRAFT_GRAPHS.map((item) => [item.id, item]));
