import { describe, expect, test } from "bun:test";
import { isValidElement } from "react";

import { resolveAddNodeMenuCommands, type AddNodeMenuContext } from "@oc/lib/canvas/tool-registry";
import { CanvasNodeType } from "@oc/types/canvas";

const handlers = {} as AddNodeMenuContext["handlers"];
const CREATE_SHOT_NODE_IDS = [CanvasNodeType.Script, CanvasNodeType.Image, CanvasNodeType.Video, CanvasNodeType.Audio];
const WORKBENCH_IDS = ["style", "library", "director"];

describe("compact create menu", () => {
    test("创作精简菜单只露出片原语，加工节点进更多或隐藏", () => {
        const commands = resolveAddNodeMenuCommands({ workspaceMode: "professional", isProjectLinked: false, compactCreateMenu: true, handlers });
        const nodeIds = commands.filter((command) => command.section === "node").map((command) => command.id);
        const moreIds = commands.filter((command) => command.section === "extension").map((command) => command.id);
        const workbenchIds = commands.filter((command) => command.section === "project").map((command) => command.id);
        expect(nodeIds).toEqual(CREATE_SHOT_NODE_IDS);
        expect(workbenchIds).toEqual(WORKBENCH_IDS);
        expect(moreIds).toContain(CanvasNodeType.Config);
        expect(moreIds).toContain(CanvasNodeType.Text);
        expect(moreIds).toContain(CanvasNodeType.Drawing);
        expect(moreIds).toContain(CanvasNodeType.Frame);
        expect(moreIds).toContain(CanvasNodeType.Panorama);
        expect(moreIds).toContain("folder");
        expect(commands.some((command) => command.id === CanvasNodeType.ArtCritique)).toBe(false);
        expect(commands.some((command) => command.id === CanvasNodeType.Markdown)).toBe(false);
        expect(commands.some((command) => command.id === "upload" && command.section === "resource")).toBe(true);
    });

    test("专业画布创作节点只含片原语，货架等入口进工作台", () => {
        const commands = resolveAddNodeMenuCommands({ workspaceMode: "professional", isProjectLinked: false, handlers });
        expect(commands.filter((command) => command.section === "node").map((command) => command.id)).toEqual(CREATE_SHOT_NODE_IDS);
        expect(commands.filter((command) => command.section === "project").map((command) => command.id)).toEqual(WORKBENCH_IDS);
        expect(commands.some((command) => command.id === CanvasNodeType.ArtCritique && command.section === "extension")).toBe(true);
        expect(commands.some((command) => command.id === CanvasNodeType.Text && command.section === "extension")).toBe(true);
        expect(commands.some((command) => command.id === CanvasNodeType.Config && command.section === "extension")).toBe(true);
        expect(commands.some((command) => command.id === CanvasNodeType.Drawing && command.section === "extension")).toBe(true);
        expect(commands.some((command) => command.id === CanvasNodeType.Frame && command.section === "extension")).toBe(true);
        expect(commands.some((command) => command.id === "folder" && command.section === "extension")).toBe(true);
    });

    test("已关联项目时工作台不出现项目画风", () => {
        const commands = resolveAddNodeMenuCommands({ workspaceMode: "professional", isProjectLinked: true, handlers });
        expect(commands.filter((command) => command.section === "project").map((command) => command.id)).toEqual(["library", "director"]);
    });

    test("workspaceMode simple/professional 不再分流菜单", () => {
        const ids = (mode: "simple" | "professional") => resolveAddNodeMenuCommands({ workspaceMode: mode, isProjectLinked: false, handlers }).map((command) => `${command.section}:${command.id}`);
        expect(ids("simple")).toEqual(ids("professional"));
    });

    test("创作菜单每个命令都有可渲染图标", () => {
        const commands = resolveAddNodeMenuCommands({ workspaceMode: "professional", isProjectLinked: false, handlers });
        expect(commands.length).toBeGreaterThan(0);
        for (const command of commands) {
            expect(isValidElement(command.icon), command.id).toBe(true);
        }
    });

    test("注册表节点使用各自的 lucide 图标而不是统一兜底", () => {
        const commands = resolveAddNodeMenuCommands({ workspaceMode: "professional", isProjectLinked: false, handlers });
        const iconName = (id: string) => {
            const command = commands.find((item) => item.id === id);
            expect(isValidElement(command?.icon), id).toBe(true);
            const type = isValidElement(command?.icon) ? command.icon.type : null;
            return typeof type === "object" && type && "displayName" in type ? String(type.displayName) : String(type);
        };
        expect(iconName(CanvasNodeType.Text)).toBe("Type");
        expect(iconName(CanvasNodeType.Image)).toBe("Image");
        expect(iconName(CanvasNodeType.Video)).toBe("Video");
        expect(iconName(CanvasNodeType.Audio)).toBe("Music2");
        expect(iconName(CanvasNodeType.Config)).toBe("Settings2");
        expect(iconName(CanvasNodeType.Script)).toBe("Clapperboard");
        expect(iconName(CanvasNodeType.Frame)).toBe("PanelTop");
        expect(iconName(CanvasNodeType.Drawing)).toBe("Pencil");
    });
});
