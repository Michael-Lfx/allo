import { describe, expect, test } from "bun:test";
import i18n from "i18next";

import { CANVAS_AGENT_ADVERTISED_TOOLS } from "@oc/lib/canvas/canvas-agent-harness";
import { defaultConfig } from "@oc/stores/use-config-store";
import type { CanvasAgentOp, CanvasAgentSnapshot } from "@oc/lib/canvas/canvas-agent-ops";
import { CanvasNodeType } from "@oc/types/canvas";

// 断言只关心 add_node 的 payload 面；CanvasAgentOp 是判别联合，直接 toMatchObject 会因其它成员无 metadata 报类型错。
type AddNodeOp = Extract<CanvasAgentOp, { type: "add_node" }>;
import {
    ONLINE_AGENT_PROMPT,
    ONLINE_AGENT_TOOLS,
    describeCanvasSnapshot,
    describeOnlineToolProgress,
    inspectCanvasOps,
    isWritableToolCall,
    onlineToolToOps,
    parseToolArguments,
    previewOnlineToolCalls,
    requireOps,
    summarizeToolCalls,
    toolCallToResponseInput,
    toolCallsFromDetail,
} from "./canvas-online-agent-tools";

// canvasT 依赖默认 i18n 实例；测试环境无应用入口，初始化后缺失 key 走 defaultValue 兜底。
i18n.init({ lng: "zh-CN", fallbackLng: "zh-CN", resources: { "zh-CN": { translation: {} } }, initImmediate: false });

// 项目本地 bun:test 类型声明只覆盖部分 matcher；抛错断言统一走 throws 辅助。
function throws(fn: () => unknown) {
    try {
        fn();
        return false;
    } catch {
        return true;
    }
}

function snapshot(): CanvasAgentSnapshot {
    return {
        projectId: "project-1",
        title: "测试画布",
        nodes: [
            { id: "n1", type: CanvasNodeType.Text, title: "剧本", position: { x: 0, y: 0 }, width: 340, height: 240 },
            { id: "n2", type: CanvasNodeType.Image, title: "封面", position: { x: 340, y: 0 }, width: 420, height: 300 },
        ],
        connections: [{ id: "c1", fromNodeId: "n1", toNodeId: "n2" }],
        selectedNodeIds: ["n1"],
        viewport: { x: 0, y: 0, k: 1 },
    };
}

describe("parseToolArguments", () => {
    test("parses a JSON object payload", () => {
        expect(parseToolArguments('{"id":"n1","count":2}')).toEqual({ id: "n1", count: 2 });
    });

    test("rejects non-object JSON and malformed input", () => {
        expect(throws(() => parseToolArguments("[1,2]"))).toBe(true);
        expect(throws(() => parseToolArguments("not json"))).toBe(true);
        expect(throws(() => parseToolArguments('"text"'))).toBe(true);
    });

    test("accepts object payloads and repairs truncated workflow JSON", () => {
        expect(parseToolArguments({ prompt: "一只猫" })).toEqual({ prompt: "一只猫" });
        expect(parseToolArguments("```json\n{\"prompt\":\"一只猫\"}\n```")).toEqual({ prompt: "一只猫" });
        expect(parseToolArguments('{"nodes":[{"ref":"a","kind":"text","title":"剧本"')).toEqual({
            nodes: [{ ref: "a", kind: "text", title: "剧本" }],
        });
    });
});

describe("onlineToolToOps", () => {
    test("canvas_apply_ops passes validated ops through", () => {
        const ops = onlineToolToOps("canvas_apply_ops", { ops: [{ type: "select_nodes", ids: ["n1"] }] }, snapshot(), defaultConfig);
        expect(ops).toEqual([{ type: "select_nodes", ids: ["n1"] }]);
        expect(throws(() => onlineToolToOps("canvas_apply_ops", { ops: "oops" }, snapshot(), defaultConfig))).toBe(true);
    });

    test("canvas_create_text_node builds a text add_node with content", () => {
        const ops = onlineToolToOps("canvas_create_text_node", { text: "标题", x: 10, y: 20, title: "新文本" }, snapshot(), defaultConfig);
        expect(ops).toEqual([{ type: "add_node", id: "", nodeType: CanvasNodeType.Text, title: "新文本", position: { x: 10, y: 20 }, width: undefined, height: undefined, metadata: { content: "标题", status: "success", fontSize: 14 } }]);
    });

    test("canvas_generate_image builds a full flow with autoRun", () => {
        const ops = onlineToolToOps("canvas_generate_image", { prompt: "一只猫" }, snapshot(), defaultConfig);
        expect(ops.map((op) => op.type)).toEqual(["add_node", "add_node", "connect_nodes", "select_nodes", "run_generation"]);
        const [textNode, targetNode, , , run] = ops;
        expect(textNode as AddNodeOp).toMatchObject({ nodeType: CanvasNodeType.Text, metadata: { content: "一只猫" } });
        expect(targetNode as AddNodeOp).toMatchObject({ nodeType: CanvasNodeType.Image, metadata: { generationMode: "image" } });
        expect(String(((targetNode as AddNodeOp).metadata as Record<string, unknown> | undefined)?.prompt ?? "")).toMatch(/@\[node:/);
        expect(run).toMatchObject({ type: "run_generation", mode: "image" });
    });

    test("canvas_generate_text defaults mode to text", () => {
        const ops = onlineToolToOps("canvas_generate_text", { prompt: "文案" }, snapshot(), defaultConfig);
        expect(ops[1]).toMatchObject({ nodeType: CanvasNodeType.Text, metadata: { generationMode: "text" } });
    });

    test("canvas_move_nodes resolves relative offsets from snapshot", () => {
        const ops = onlineToolToOps("canvas_move_nodes", { items: [{ id: "n1", dx: 10, dy: 5 }] }, snapshot(), defaultConfig);
        expect(ops).toEqual([{ type: "update_node", id: "n1", patch: { position: { x: 10, y: 5 } } }]);
    });

    test("canvas_create_workflow builds typed nodes and connections", () => {
        const ops = onlineToolToOps("canvas_create_workflow", {
            title: "角色流水线",
            nodes: [
                { ref: "script", kind: "script", title: "剧本", content: "开场" },
                { ref: "cards", kind: "character_cards", title: "角色卡", prompt: "拆角色" },
            ],
            edges: [{ from: "script", to: "cards" }],
        }, snapshot(), defaultConfig);
        expect(ops.some((op) => op.type === "add_node" && op.nodeType === CanvasNodeType.Script)).toBe(true);
        expect(ops.some((op) => op.type === "add_node" && op.nodeType === CanvasNodeType.Image)).toBe(true);
        expect(ops.some((op) => op.type === "connect_nodes")).toBe(true);
        expect(ops.some((op) => op.type === "select_nodes")).toBe(true);
    });

    test("canvas_apply compiles the same semantic graph as create_workflow", () => {
        const ops = onlineToolToOps("canvas_apply", {
            nodes: [
                { ref: "script", kind: "script", title: "剧本", content: "小猫出门" },
                { ref: "shot1", kind: "image", title: "镜头1", prompt: "出门" },
                { ref: "video", kind: "video", title: "成片", prompt: "一天", seconds: "6" },
            ],
            edges: [
                { from: "script", to: "shot1" },
                { from: "shot1", to: "video" },
            ],
        }, snapshot(), defaultConfig);
        const video = ops.find((op): op is AddNodeOp => op.type === "add_node" && op.nodeType === CanvasNodeType.Video);
        expect(String(video?.metadata?.prompt || "")).toMatch(/@\[node:/);
        expect(video?.metadata?.videoStartFrameNodeId).toBeTruthy();
    });

    test("canvas_create_text_nodes rejects workflow-like copy", () => {
        expect(throws(() => onlineToolToOps("canvas_create_text_nodes", { items: [{ text: "搭一条工作流管线" }] }, snapshot(), defaultConfig))).toBe(true);
    });
});

describe("requireOps", () => {
    test("validates every op entry", () => {
        expect(requireOps([{ type: "connect_nodes", fromNodeId: "a", toNodeId: "b" }])).toEqual([{ type: "connect_nodes", id: "", fromNodeId: "a", toNodeId: "b" }]);
        expect(throws(() => requireOps([{ type: "run_generation", nodeId: 5 }]))).toBe(true);
        expect(throws(() => requireOps("nope"))).toBe(true);
    });
});

describe("inspectCanvasOps", () => {
    test("returns actionable issues instead of throwing on incomplete run_generation", () => {
        const inspected = inspectCanvasOps([{ type: "run_generation", nodeType: "video", y: 400 }]);
        expect(inspected.ops).toEqual([]);
        expect(inspected.issues[0]).toContain("nodeId 必须是非空字符串");
        expect(inspected.issues[0]).toContain("canvas_apply");
    });
});

describe("tool call helpers", () => {
    test("isWritableToolCall treats read-only tools as non-writable", () => {
        expect(isWritableToolCall({ id: "1", type: "function", function: { name: "canvas_inspect", arguments: "{}" } })).toBe(false);
        expect(isWritableToolCall({ id: "1b", type: "function", function: { name: "canvas_propose", arguments: "{}" } })).toBe(false);
        expect(isWritableToolCall({ id: "1c", type: "function", function: { name: "canvas_critique", arguments: "{}" } })).toBe(false);
        expect(isWritableToolCall({ id: "2", type: "function", function: { name: "canvas_apply", arguments: "{}" } })).toBe(true);
        expect(isWritableToolCall({ id: "3", type: "function", function: { name: "canvas_run", arguments: "{}" } })).toBe(true);
    });

    test("toolCallsFromDetail filters malformed entries", () => {
        const detail = { toolCalls: [{ id: "a", type: "function", function: { name: "canvas_get_state", arguments: "{}" } }, { id: 3 }, { type: "function", function: { name: "x", arguments: "{}" } }] };
        expect(toolCallsFromDetail(detail).map((call) => call.id)).toEqual(["a"]);
        expect(toolCallsFromDetail({})).toEqual([]);
    });

    test("toolCallsFromDetail stringifies object arguments from proxies", () => {
        const detail = { toolCalls: [{ id: "a", type: "function", function: { name: "canvas_create_workflow", arguments: { nodes: [{ ref: "script" }] } } }] };
        expect(toolCallsFromDetail(detail)).toEqual([{ id: "a", type: "function", function: { name: "canvas_create_workflow", arguments: '{"nodes":[{"ref":"script"}]}' } }]);
    });

    test("toolCallToResponseInput maps to a function_call message", () => {
        expect(toolCallToResponseInput({ id: "a", type: "function", function: { name: "canvas_inspect", arguments: "{}" } })).toEqual({ type: "function_call", call_id: "a", name: "canvas_inspect", arguments: "{}" });
    });

    test("summarizeToolCalls joins localized labels", () => {
        const summary = summarizeToolCalls([{ id: "a", type: "function", function: { name: "canvas_inspect", arguments: "{}" } }, { id: "b", type: "function", function: { name: "canvas_apply", arguments: "{}" } }]);
        expect(summary).toContain("观察画布");
        expect(summary).toContain("更新画布");
    });

    test("previewOnlineToolCalls ignores read-only tools and counts generation ops", () => {
        const calls = [
            { id: "a", type: "function" as const, function: { name: "canvas_inspect", arguments: "{}" } },
            { id: "b", type: "function" as const, function: { name: "canvas_generate_image", arguments: JSON.stringify({ prompt: "一只猫" }) } },
        ];
        const impact = previewOnlineToolCalls(calls, snapshot(), defaultConfig);
        expect(impact.operationCount).toBeGreaterThanOrEqual(5);
        expect(impact.generationCount).toBe(1);
        expect(impact.spend).toBe(true);
        expect(impact.stages.length).toBeGreaterThan(0);
    });
});

describe("describeCanvasSnapshot", () => {
    test("counts nodes and connections by type", () => {
        const text = describeCanvasSnapshot(snapshot());
        expect(text).toContain("2 个节点");
        expect(text).toContain("1 条连线");
        expect(text).toContain("文本 1 个");
        expect(text).toContain("图片 1 个");
    });
});

describe("ONLINE_AGENT_TOOLS", () => {
    test("exposes intent tools instead of a flat canvas_* dump", () => {
        const names = ONLINE_AGENT_TOOLS.map((tool) => tool.function.name);
        expect(names).toEqual([
            "canvas_list_skills",
            "canvas_get_skill",
            "canvas_inspect",
            "canvas_propose",
            "canvas_apply",
            "canvas_run",
            "canvas_critique",
            "canvas_repair",
        ]);
        expect(names).not.toContain("canvas_create_cinematic_session");
        expect(names).not.toContain("canvas_apply_ops");
        expect(names).not.toContain("canvas_create_workflow");
        expect(ONLINE_AGENT_TOOLS.every((tool) => tool.function.parameters.additionalProperties === false)).toBe(true);
        expect(names).toEqual([...CANVAS_AGENT_ADVERTISED_TOOLS]);
    });
});

describe("ONLINE_AGENT_PROMPT", () => {
    test("describes an observe-act loop without hardcoded pipelines", () => {
        expect(ONLINE_AGENT_PROMPT).toContain("感知—行动—观察");
        expect(ONLINE_AGENT_PROMPT).toContain("canvas_apply");
        expect(ONLINE_AGENT_PROMPT).toContain("自己根据用户目标设计图");
        expect(ONLINE_AGENT_PROMPT).not.toContain("首轮必须调用 canvas_get_context");
        expect(ONLINE_AGENT_PROMPT).not.toContain("必须使用 canvas_create_workflow");
        expect(ONLINE_AGENT_PROMPT).not.toContain("关键帧图 prompt 必须来自分镜行");
        expect(describeOnlineToolProgress("canvas_apply")).toContain("更新画布");
    });
});
