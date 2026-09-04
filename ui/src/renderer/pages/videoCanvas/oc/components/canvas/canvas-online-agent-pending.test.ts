import { describe, expect, test } from "bun:test";

import type { CanvasAssistantSession } from "@oc/types/canvas";
import { dropPendingOnlineToolContext, resolvePendingOnlineToolContext, stashPendingOnlineToolContext } from "./canvas-online-agent-pending";

function session(): CanvasAssistantSession {
    return {
        id: "sess-1",
        title: "画布对话",
        createdAt: "2026-09-01T00:00:00.000Z",
        updatedAt: "2026-09-01T00:01:00.000Z",
        messages: [
            { id: "u1", role: "user", text: "搭一条角色流水线" },
            { id: "a1", role: "assistant", text: "准备执行工具，等待确认。" },
            {
                id: "t1",
                role: "tool",
                title: "确认工具调用",
                text: "创建工作流",
                detail: {
                    status: "pending",
                    step: 1,
                    assistantId: "a1",
                    toolCalls: [{ id: "call-1", type: "function", function: { name: "canvas_create_workflow", arguments: { nodes: [{ ref: "script", kind: "text", title: "剧本" }] } } }],
                },
            },
        ],
    };
}

describe("pending online tool context", () => {
    test("reconstructs tool calls and assistant id after the in-memory stash is lost", () => {
        const current = session();
        const detail = current.messages[2].detail as Record<string, unknown>;
        dropPendingOnlineToolContext("t1");
        const restored = resolvePendingOnlineToolContext("t1", detail, current);
        expect(restored?.assistantId).toBe("a1");
        expect(restored?.toolCalls).toEqual([{ id: "call-1", type: "function", function: { name: "canvas_create_workflow", arguments: '{"nodes":[{"ref":"script","kind":"text","title":"剧本"}]}' } }]);
        expect(restored?.messages).toEqual([]);
    });

    test("prefers the live stash when the panel remounts in the same page load", () => {
        stashPendingOnlineToolContext("t1", {
            messages: [{ role: "system", content: "agent" }],
            toolCalls: [{ id: "call-1", type: "function", function: { name: "canvas_create_workflow", arguments: "{}" } }],
            assistantId: "live-assistant",
            step: 2,
        });
        const restored = resolvePendingOnlineToolContext("t1", {}, session());
        expect(restored?.assistantId).toBe("live-assistant");
        expect(restored?.step).toBe(2);
        expect(restored?.messages).toEqual([{ role: "system", content: "agent" }]);
        dropPendingOnlineToolContext("t1");
    });
});
