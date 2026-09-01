import { describe, expect, test } from "bun:test";

import { CanvasNodeType, type CanvasAssistantSession } from "@oc/types/canvas";
import type { CanvasProject } from "@oc/stores/canvas/use-canvas-store";
import { parsePersistedChatSessions, persistableChatSessions, projectToCanvasDocument } from "./canvasChatPersist";

const session: CanvasAssistantSession = {
    id: "sess-1",
    title: "噜噜跳舞",
    createdAt: "2026-09-01T00:00:00.000Z",
    updatedAt: "2026-09-01T00:01:00.000Z",
    messages: [
        {
            id: "m1",
            role: "user",
            text: "噜噜跳舞",
            references: [{ id: "img-1", type: CanvasNodeType.Image, title: "参考", dataUrl: "data:image/png;base64,AAAA" }],
        },
        {
            id: "m2",
            role: "tool",
            title: "创建工作流",
            text: "画布已更新",
            detail: {
                status: "completed",
                name: "canvas_create_workflow",
                result: { ok: true, message: "画布已更新", data: { snapshot: { nodes: [1, 2, 3] } } },
            },
        },
    ],
};

describe("canvas chat persist", () => {
    test("strips dataUrls and bulky tool snapshots while keeping the transcript", () => {
        const persisted = persistableChatSessions([session]);
        expect(persisted).toHaveLength(1);
        expect(persisted[0].messages[0].references?.[0].dataUrl).toBeUndefined();
        expect(persisted[0].messages[0].references?.[0].id).toBe("img-1");
        const detail = persisted[0].messages[1].detail as { result?: { data?: unknown; message?: string } };
        expect(detail.result?.message).toBe("画布已更新");
        expect(detail.result?.data).toBeUndefined();
    });

    test("round-trips sessions through the canvas document", () => {
        const project = {
            id: "p1",
            title: "画布",
            createdAt: session.createdAt,
            updatedAt: session.updatedAt,
            nodes: [],
            connections: [],
            chatSessions: [session],
            activeChatId: "sess-1",
            backgroundMode: "lines",
            showImageInfo: false,
            viewport: { x: 0, y: 0, k: 1 },
            directorScenes: [],
        } as CanvasProject;
        const doc = projectToCanvasDocument(project);
        expect(doc.chatSessions).toHaveLength(1);
        expect(doc.activeChatId).toBe("sess-1");
        const restored = parsePersistedChatSessions(doc.chatSessions);
        expect(restored[0].id).toBe("sess-1");
        expect(restored[0].messages.map((message) => message.role)).toEqual(["user", "tool"]);
        expect(parsePersistedChatSessions(undefined)).toEqual([]);
    });

    test("drops empty sessions so a new chat does not wipe history on save", () => {
        expect(persistableChatSessions([{ ...session, id: "empty", messages: [] }])).toEqual([]);
    });

    test("keeps every session with messages when starting a new blank chat", () => {
        const older: CanvasAssistantSession = {
            ...session,
            id: "sess-2",
            title: "第二个镜头",
            messages: [{ id: "n1", role: "user", text: "再做一个镜头" }],
        };
        const blank: CanvasAssistantSession = { ...session, id: "sess-new", title: "新对话", messages: [] };
        const project = {
            id: "p1",
            title: "画布",
            createdAt: session.createdAt,
            updatedAt: session.updatedAt,
            nodes: [],
            connections: [],
            chatSessions: [blank, session, older],
            activeChatId: "sess-new",
            backgroundMode: "lines",
            showImageInfo: false,
            viewport: { x: 0, y: 0, k: 1 },
            directorScenes: [],
        } as CanvasProject;
        const persisted = persistableChatSessions(project.chatSessions);
        expect(persisted.map((item) => item.id)).toEqual(["sess-1", "sess-2"]);
        const doc = projectToCanvasDocument(project);
        expect(parsePersistedChatSessions(doc.chatSessions).map((item) => item.id)).toEqual(["sess-1", "sess-2"]);
        expect(doc.activeChatId).toBe("sess-1");
    });
});
