import type { CanvasAgentSnapshot } from "./canvas-agent-ops";
import type { CanvasNodeData } from "@oc/types/canvas";

const SHORT_ID_PATTERN = /^n(\d+)$/i;
const NODE_MENTION_PATTERN = /@\[node:([^\]]+)\]/g;
const SHORT_MENTION_PATTERN = /(^|[^A-Za-z0-9_])@(n\d+)\b/gi;

export type CanvasAgentAliasMap = {
    byShortId: Map<string, string>;
    byNodeId: Map<string, string>;
};

/** 优先使用已落盘的 agentAlias；其余按节点 id 排序补号，避免加节点后 n1 换人。 */
export function buildCanvasAgentAliasMap(nodes: CanvasNodeData[]): CanvasAgentAliasMap {
    const byShortId = new Map<string, string>();
    const byNodeId = new Map<string, string>();
    const used = new Set<string>();
    for (const node of nodes) {
        const alias = normalizeAlias(node.metadata?.agentAlias);
        if (!alias || used.has(alias) || byShortId.has(alias)) continue;
        used.add(alias);
        byShortId.set(alias, node.id);
        byNodeId.set(node.id, alias);
    }
    let next = 1;
    [...nodes]
        .map((node) => node.id)
        .filter(Boolean)
        .sort((left, right) => left.localeCompare(right))
        .forEach((id) => {
            if (byNodeId.has(id)) return;
            while (used.has(`n${next}`)) next += 1;
            const shortId = `n${next}`;
            next += 1;
            used.add(shortId);
            byShortId.set(shortId, id);
            byNodeId.set(id, shortId);
        });
    return { byShortId, byNodeId };
}

export function stampCanvasAgentAliases(nodes: CanvasNodeData[]): CanvasNodeData[] {
    const aliases = buildCanvasAgentAliasMap(nodes);
    return nodes.map((node) => {
        const alias = canvasAgentShortId(node.id, aliases);
        if (node.metadata?.agentAlias === alias) return node;
        return { ...node, metadata: { ...node.metadata, agentAlias: alias } };
    });
}

function normalizeAlias(value: unknown) {
    if (typeof value !== "string") return "";
    const alias = value.trim().toLowerCase();
    return SHORT_ID_PATTERN.test(alias) ? alias : "";
}

export function canvasAgentShortId(nodeId: string, aliases: CanvasAgentAliasMap) {
    return aliases.byNodeId.get(nodeId) || nodeId;
}

export function resolveCanvasAgentNodeId(snapshot: CanvasAgentSnapshot, token: string, aliases = buildCanvasAgentAliasMap(snapshot.nodes)) {
    const value = token.trim();
    if (!value) return undefined;
    if (snapshot.nodes.some((node) => node.id === value)) return value;
    const shortMatch = value.match(SHORT_ID_PATTERN);
    if (shortMatch) return aliases.byShortId.get(`n${shortMatch[1]}`);
    return aliases.byShortId.get(value.toLocaleLowerCase());
}

export function resolveCanvasAgentNodeIds(snapshot: CanvasAgentSnapshot, tokens: string[]) {
    const aliases = buildCanvasAgentAliasMap(snapshot.nodes);
    const ids: string[] = [];
    const missing: string[] = [];
    for (const token of tokens) {
        const id = resolveCanvasAgentNodeId(snapshot, token, aliases);
        if (id) ids.push(id);
        else missing.push(token);
    }
    return { ids: [...new Set(ids)], missing };
}

export function parseCanvasAgentMentionTokens(text: string) {
    const tokens: string[] = [];
    for (const match of text.matchAll(NODE_MENTION_PATTERN)) {
        if (match[1]) tokens.push(match[1]);
    }
    for (const match of text.matchAll(SHORT_MENTION_PATTERN)) {
        if (match[2]) tokens.push(match[2]);
    }
    return [...new Set(tokens)];
}

export function canvasAgentNodeChangeKind(previous: CanvasNodeData | undefined, next: CanvasNodeData) {
    if (!previous) return "new" as const;
    return nodeSignature(previous) === nodeSignature(next) ? undefined : ("modified" as const);
}

function nodeSignature(node: CanvasNodeData) {
    return JSON.stringify({
        type: node.type,
        title: node.title,
        position: node.position,
        width: node.width,
        height: node.height,
        parentId: node.parentId,
        status: node.metadata?.status,
        prompt: node.metadata?.prompt || node.metadata?.composerContent,
        content: typeof node.metadata?.content === "string" ? node.metadata.content.slice(0, 80) : "",
        model: node.metadata?.model,
        taskId: node.metadata?.taskId,
        taskStatus: node.metadata?.taskStatus,
    });
}
