import { createStoryboardRow } from "@oc/lib/canvas/canvas-project-domain";
import type { StoryboardRow } from "@oc/types/canvas";

export type CanvasWorkflowShotInput = {
    plotDescription?: string;
    dialogue?: string;
    durationSeconds?: number;
    imagePrompt?: string;
    videoPrompt?: string;
};

const SCENE_SPLIT = /(?:^|\n)\s*(?:场景|镜头|分镜|Shot|Scene)\s*[0-9一二三四五六七八九十]+[：:.\s、)）-]*/gi;
const DURATION_IN_HEAD = /^\s*[（(]?\s*(\d+(?:\.\d+)?)\s*(?:s|S|秒)\s*[)）]?\s*[：:.\-–—]?\s*/;
const DURATION_ANYWHERE = /(\d+(?:\.\d+)?)\s*(?:s|S|秒)/;
const DIALOGUE_SPLIT = /(?:台词|旁白|对白)\s*[：:]/;

export function resolveWorkflowStoryboardRows(input: {
    shots?: CanvasWorkflowShotInput[];
    content?: string;
    prompt?: string;
    imageCount?: number;
    totalDurationSeconds?: number;
}): StoryboardRow[] {
    const parsed = input.shots?.length
        ? input.shots.map(shotFromInput)
        : parseShotsFromText(input.content || input.prompt || "");
    const imageCount = Math.max(0, Math.floor(input.imageCount || 0));
    const shots = alignShotCount(parsed, imageCount, input.content || input.prompt || "");
    const durations = fitShotDurations(shots.map((shot) => shot.durationSeconds), shots.length, input.totalDurationSeconds);
    return shots.map((shot, index) => createStoryboardRow(index + 1, {
        durationSeconds: durations[index],
        plotDescription: shot.plotDescription,
        dialogue: shot.dialogue,
        imageGenerationPrompt: shot.imagePrompt || shot.plotDescription,
        videoMotionPrompt: shot.videoPrompt,
        status: "idle",
    }));
}

export function parseDurationSeconds(value: unknown): number | undefined {
    if (typeof value === "number" && Number.isFinite(value) && value > 0) return Math.max(1, Math.round(value));
    if (typeof value !== "string") return undefined;
    const match = value.trim().match(/^(\d+(?:\.\d+)?)/);
    if (!match) return undefined;
    const parsed = Number(match[1]);
    return Number.isFinite(parsed) && parsed > 0 ? Math.max(1, Math.round(parsed)) : undefined;
}

function shotFromInput(shot: CanvasWorkflowShotInput) {
    return {
        plotDescription: String(shot.plotDescription || "").trim(),
        dialogue: String(shot.dialogue || "").trim(),
        durationSeconds: parseDurationSeconds(shot.durationSeconds),
        imagePrompt: String(shot.imagePrompt || "").trim(),
        videoPrompt: String(shot.videoPrompt || "").trim(),
    };
}

function parseShotsFromText(text: string) {
    const source = text.trim();
    if (!source) return [];
    const parts = splitScenes(source);
    return parts.map((part) => parseShotBlock(part));
}

function splitScenes(text: string) {
    const matches = [...text.matchAll(new RegExp(SCENE_SPLIT.source, SCENE_SPLIT.flags))];
    if (matches.length >= 2) {
        return matches.map((match, index) => {
            const start = (match.index || 0) + match[0].length;
            const end = index + 1 < matches.length ? matches[index + 1].index || text.length : text.length;
            return text.slice(start, end).trim();
        }).filter(Boolean);
    }
    const numbered = text.split(/(?:^|\n)\s*\d+\s*[.、．)]\s+/).map((part) => part.trim()).filter(Boolean);
    if (numbered.length >= 2 && numbered[0] !== text.trim()) return numbered;
    const paragraphs = text.split(/\n\s*\n/).map((part) => part.trim()).filter(Boolean);
    return paragraphs.length >= 2 ? paragraphs : [text.trim()];
}

function parseShotBlock(block: string) {
    let body = block.trim();
    let durationSeconds = parseDurationSeconds(body.match(DURATION_IN_HEAD)?.[1]);
    if (durationSeconds) body = body.replace(DURATION_IN_HEAD, "").trim();
    else {
        const anywhere = body.match(DURATION_ANYWHERE);
        durationSeconds = parseDurationSeconds(anywhere?.[1]);
    }
    const dialogueParts = body.split(DIALOGUE_SPLIT);
    const plotDescription = (dialogueParts[0] || "").trim();
    const dialogue = dialogueParts.slice(1).join(" ").trim();
    return {
        plotDescription,
        dialogue,
        durationSeconds,
        imagePrompt: plotDescription,
        videoPrompt: "",
    };
}

function alignShotCount(shots: ReturnType<typeof parseShotsFromText>, imageCount: number, fallbackText: string) {
    const target = Math.max(shots.length, imageCount, shots.length || imageCount || (fallbackText.trim() ? 1 : 0));
    if (!target) return [];
    if (shots.length === target) return shots;
    if (shots.length > target) return shots.slice(0, target);
    if (shots.length === 1 && target > 1) {
        const chunks = splitEvenly(shots[0].plotDescription || fallbackText, target);
        return chunks.map((plotDescription, index) => ({
            ...shots[0],
            plotDescription,
            imagePrompt: plotDescription,
            durationSeconds: shots[0].durationSeconds,
            dialogue: index === 0 ? shots[0].dialogue : "",
        }));
    }
    const extras = Array.from({ length: target - shots.length }, (_, index) => ({
        plotDescription: shots.length ? `${shots[shots.length - 1]?.plotDescription || fallbackText}（镜头 ${shots.length + index + 1}）` : fallbackText.trim(),
        dialogue: "",
        durationSeconds: undefined as number | undefined,
        imagePrompt: "",
        videoPrompt: "",
    }));
    return [...shots, ...extras];
}

function splitEvenly(text: string, count: number) {
    const sentences = text.split(/(?<=[。！？!?\n])/).map((part) => part.trim()).filter(Boolean);
    if (sentences.length >= count) {
        const size = Math.ceil(sentences.length / count);
        return Array.from({ length: count }, (_, index) => sentences.slice(index * size, (index + 1) * size).join("").trim() || text);
    }
    return Array.from({ length: count }, (_, index) => text.trim() || `镜头 ${index + 1}`);
}

function fitShotDurations(parsed: Array<number | undefined>, count: number, totalDurationSeconds?: number) {
    const total = totalDurationSeconds && totalDurationSeconds > 0 ? Math.max(count, Math.round(totalDurationSeconds)) : undefined;
    const known = parsed.map((value) => (value && value > 0 ? Math.max(1, Math.round(value)) : 0));
    const knownSum = known.reduce((sum, value) => sum + value, 0);
    const missing = known.filter((value) => !value).length;
    if (!total) {
        const fallback = count > 1 ? 2 : 6;
        return known.map((value) => value || fallback);
    }
    if (!missing && knownSum === total) return known;
    if (!missing && knownSum > 0 && knownSum !== total) {
        const scaled = known.map((value) => Math.max(1, Math.round((value / knownSum) * total)));
        return fixDurationRemainder(scaled, total);
    }
    const remaining = Math.max(count, total - knownSum);
    const even = Math.max(1, Math.floor(remaining / Math.max(1, missing || count)));
    const next = known.map((value) => value || even);
    return fixDurationRemainder(next, total);
}

function fixDurationRemainder(durations: number[], total: number) {
    const next = durations.map((value) => Math.max(1, value));
    let diff = total - next.reduce((sum, value) => sum + value, 0);
    let index = next.length - 1;
    while (diff !== 0 && next.length) {
        const step = diff > 0 ? 1 : -1;
        if (step < 0 && next[index] <= 1) {
            index = (index + next.length - 1) % next.length;
            if (next.every((value) => value <= 1) && diff < 0) break;
            continue;
        }
        next[index] += step;
        diff -= step;
        index = (index + next.length - 1) % next.length;
    }
    return next;
}
