import { describe, expect, test } from "bun:test";

import { resolveAddNodeMenuCommands, type AddNodeMenuContext } from "@oc/lib/canvas/tool-registry";
import { CanvasNodeType } from "@oc/types/canvas";

const handlers = {} as AddNodeMenuContext["handlers"];

describe("compact create menu", () => {
    test("创作精简菜单只露出片原语，加工节点进更多或隐藏", () => {
        const commands = resolveAddNodeMenuCommands({ workspaceMode: "professional", isProjectLinked: false, compactCreateMenu: true, handlers });
        const nodeIds = commands.filter((command) => command.section === "node").map((command) => command.id);
        const moreIds = commands.filter((command) => command.section === "extension").map((command) => command.id);
        expect(nodeIds).toEqual(["style", CanvasNodeType.Script, CanvasNodeType.Image, CanvasNodeType.Video, "director", CanvasNodeType.Audio]);
        expect(moreIds).toContain(CanvasNodeType.Config);
        expect(moreIds).toContain(CanvasNodeType.Text);
        expect(moreIds).toContain(CanvasNodeType.Panorama);
        expect(moreIds).toContain("folder");
        expect(commands.some((command) => command.id === CanvasNodeType.ArtCritique)).toBe(false);
        expect(commands.some((command) => command.id === CanvasNodeType.Markdown)).toBe(false);
        expect(commands.some((command) => command.id === "upload" && command.section === "resource")).toBe(true);
    });

    test("专业画布菜单仍露出审美批改与创作网格原文案分区", () => {
        const commands = resolveAddNodeMenuCommands({ workspaceMode: "professional", isProjectLinked: false, handlers });
        expect(commands.some((command) => command.id === CanvasNodeType.ArtCritique && command.section === "extension")).toBe(true);
        expect(commands.some((command) => command.id === CanvasNodeType.Text && command.section === "node")).toBe(true);
    });

    test("workspaceMode simple/professional 不再分流菜单", () => {
        const ids = (mode: "simple" | "professional") => resolveAddNodeMenuCommands({ workspaceMode: mode, isProjectLinked: false, handlers }).map((command) => `${command.section}:${command.id}`);
        expect(ids("simple")).toEqual(ids("professional"));
    });
});
