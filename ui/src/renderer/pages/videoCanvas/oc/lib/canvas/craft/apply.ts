import { createCanvasNode } from "@oc/lib/canvas/canvas-project-domain";
import { CanvasNodeType, type CanvasConnection, type CanvasNodeData, type CanvasSkillCategory, type CanvasSkillSnapshot, type CanvasWorkflowKind, type Position } from "@oc/types/canvas";

import type { CraftGraphBuild, CraftPlaybook, CraftRecipe, RecipeApplyPatch } from "./types";
import { GRAPH_BY_ID } from "./graphs";
import { recipeToken } from "./tokens";

export function recipeApplyPatch(recipe: CraftRecipe, prompt: string): RecipeApplyPatch {
    const next: RecipeApplyPatch = { prompt: insertOrKeep(prompt, recipe.id) };
    if (recipe.cameraMoveId) {
        next.cameraMoveId = recipe.cameraMoveId;
        next.cameraMovePrompt = recipe.prompt;
    }
    return next;
}

function insertOrKeep(prompt: string, recipeId: string) {
    const token = recipeToken(recipeId);
    if (prompt.includes(token)) return prompt;
    const trimmed = prompt.replace(/(^|\s)\/[\p{L}\p{N}_-]*$/u, "$1").trimEnd();
    return trimmed ? `${trimmed} ${token}` : token;
}

function playbookCategory(category: string): CanvasSkillCategory {
    if (category === "storyboard") return "storyboard";
    if (category === "product" || category === "advertising" || category === "music-mv") return "video";
    return "writing";
}

export function playbookSnapshot(playbook: CraftPlaybook, template: string, coverUrl?: string): CanvasSkillSnapshot {
    return {
        id: playbook.qualifiedId,
        name: playbook.title.zh,
        description: playbook.job.zh,
        category: playbookCategory(playbook.category),
        template,
        outputMode: "workflow",
        outputContract: playbook.job.zh,
        version: 1,
        tags: [playbook.category, "playbook"],
        qualifiedId: playbook.qualifiedId,
        semver: "1.0.0",
        coverUrl,
        jobToBeDone: playbook.job.zh,
    };
}

export function buildPlaybookNode(playbook: CraftPlaybook, template: string, position: Position, coverUrl?: string): CanvasNodeData {
    const snapshot = playbookSnapshot(playbook, template, coverUrl);
    const node = createCanvasNode(CanvasNodeType.Skill, position, {
        content: snapshot.jobToBeDone,
        prompt: snapshot.jobToBeDone,
        status: "success",
        skillId: playbook.qualifiedId,
        skillVersion: 1,
        projectPlaybook: true,
        skillSnapshot: snapshot,
    });
    node.title = playbook.title.zh;
    node.width = 320;
    node.height = 168;
    return node;
}

function link(from: CanvasNodeData, to: CanvasNodeData): CanvasConnection {
    return { id: `edge-${from.id}-${to.id}`, fromNodeId: from.id, toNodeId: to.id };
}

function textNode(position: Position, title: string, content: string, workflowKind?: CanvasWorkflowKind) {
    const node = createCanvasNode(CanvasNodeType.Text, position, { content, prompt: content, status: "idle", workflowKind, fontSize: 14 });
    node.title = title;
    return node;
}

function imageNode(position: Position, title: string, prompt: string) {
    const node = createCanvasNode(CanvasNodeType.Image, position, { prompt, composerContent: prompt, status: "idle", generationMode: "image" });
    node.title = title;
    return node;
}

function videoNode(position: Position, title: string, prompt: string) {
    const node = createCanvasNode(CanvasNodeType.Video, position, { prompt, composerContent: prompt, status: "idle", generationMode: "video", videoEditOperation: "text_to_video" });
    node.title = title;
    return node;
}

function audioNode(position: Position, title: string) {
    const node = createCanvasNode(CanvasNodeType.Audio, position, { status: "idle" });
    node.title = title;
    return node;
}

function scriptNode(position: Position, title: string) {
    const node = createCanvasNode(CanvasNodeType.Script, position, { status: "idle", composerContent: "" });
    node.title = title;
    return node;
}

export function buildGraphStarter(id: string, origin: Position): CraftGraphBuild | null {
    if (!GRAPH_BY_ID.has(id)) return null;
    const p = (dx: number, dy: number): Position => ({ x: origin.x + dx, y: origin.y + dy });
    if (id === "starter-character") {
        const a = textNode(p(0, 0), "角色描述", "角色身份、年龄、服装、体态", "character");
        const b = imageNode(p(420, 0), "设定图", "角色设定图：正面侧面背面与表情，身份锁定");
        const c = imageNode(p(840, 0), "三视图", "同一角色三视图，等比例");
        const d = imageNode(p(1260, 0), "表情", "同一角色表情设定，用手和脸外化情绪");
        return { nodes: [a, b, c, d], connections: [link(a, b), link(b, c), link(c, d)] };
    }
    if (id === "starter-script-film") {
        const a = scriptNode(p(0, 40), "分镜脚本");
        const b = imageNode(p(980, 0), "镜头静帧", "按分镜行生成锁定身份的静帧");
        const c = videoNode(p(980, 360), "镜头视频", "从静帧推进作表演与运镜，不改身份");
        return { nodes: [a, b, c], connections: [link(a, b), link(b, c)] };
    }
    if (id === "starter-product-ad") {
        const product = imageNode(p(0, 0), "产品图", "产品材质特写，功能可见");
        const talent = imageNode(p(0, 340), "人物", "手与产品接触，身份锁定");
        const scene = imageNode(p(460, 160), "使用场景", "产品被使用的场景静帧");
        const film = videoNode(p(920, 160), "15秒成片", "15秒产品广告，功能必须被看见");
        return { nodes: [product, talent, scene, film], connections: [link(product, scene), link(talent, scene), link(scene, film)] };
    }
    if (id === "starter-explore-3") {
        const seed = textNode(p(0, 120), "同一提示", "在此写下要对比的内容");
        const a = imageNode(p(420, 0), "路 A", "按当前画风生成");
        const b = imageNode(p(420, 280), "路 B", "换一种媒介气质，不改身份");
        const c = imageNode(p(420, 560), "路 C", "第三种光线与镜头，不改身份");
        return { nodes: [seed, a, b, c], connections: [link(seed, a), link(seed, b), link(seed, c)] };
    }
    if (id === "starter-replica") {
        const ref = videoNode(p(0, 0), "参考片", "分析这支参考的镜法");
        const notes = textNode(p(520, 0), "镜法分析", "镜法、节奏、光，不抄面孔");
        const out = videoNode(p(980, 0), "新片", "用分析到的镜法拍用户内容");
        return { nodes: [ref, notes, out], connections: [link(ref, notes), link(notes, out)] };
    }
    if (id === "starter-mv") {
        const audio = audioNode(p(0, 80), "音乐");
        const beats = textNode(p(420, 80), "节拍", "卡点与视觉母题，不堆歌词");
        const key = imageNode(p(840, 0), "关键帧", "母题静帧");
        const shot = videoNode(p(840, 340), "镜头", "按节拍卡点运动");
        return { nodes: [audio, beats, key, shot], connections: [link(audio, beats), link(beats, key), link(key, shot)] };
    }
    if (id === "starter-docu") {
        const a = imageNode(p(0, 0), "素材 A", "观察对象");
        const b = imageNode(p(0, 300), "素材 B", "环境证据");
        const shot = videoNode(p(480, 120), "观察镜", "观察式，少解说");
        const cap = textNode(p(980, 120), "字幕", "必要事实，不堆旁白");
        return { nodes: [a, b, shot, cap], connections: [link(a, shot), link(b, shot), link(shot, cap)] };
    }
    const poster = imageNode(p(0, 40), "海报", "海报构图与标题区");
    const motion = videoNode(p(480, 40), "微动", "海报微动，文字保持可读");
    return { nodes: [poster, motion], connections: [link(poster, motion)] };
}
