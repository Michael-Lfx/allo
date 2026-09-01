import { Folder, FolderOpen, Layers3, Palette, UploadCloud, UserRound } from "lucide-react";

import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { getNodeIcon, getNodeLabel } from "@oc/lib/canvas/node-registry";
import { CanvasNodeType } from "@oc/types/canvas";

import { registerAddNodeMenuCommands } from "../tool-registry";
import type { AddNodeMenuCommand } from "../tool-definition";

/** 真正创建节点的命令，文案与图标统一取自节点注册表（label 运行时解析，避免语言快照）。 */
function nodeCommand(type: CanvasNodeType, rest: Omit<AddNodeMenuCommand, "id" | "label" | "icon" | "section">): AddNodeMenuCommand {
    return { id: type, label: () => getNodeLabel(type), icon: getNodeIcon(type), section: "node", ...rest };
}

/**
 * 扩展节点命令：落在独立的「展示与加工」分区，不挤占上方调好的四列创作网格。
 */
function extensionCommand(type: CanvasNodeType, defaultOrder: number): AddNodeMenuCommand {
    return {
        id: type,
        label: () => getNodeLabel(type),
        icon: getNodeIcon(type),
        section: "extension",
        defaultOrder,
        run: (ctx) => ctx.handlers.onAddExtensionNode(type),
    };
}

export const addNodeMenuCommands: AddNodeMenuCommand[] = [
    // 创作节点：项目画风是全画布美术基线，必须出现在网格里，避免藏在标题栏被找不到。
    { id: "style", label: () => canvasT("videoCanvas.toolbar.style", "项目画风"), icon: <Palette />, badge: () => canvasT("videoCanvas.toolbar.badgeBaseline", "基线"), section: "node", defaultOrder: 5, applicable: (ctx) => !ctx.isProjectLinked, run: (ctx) => ctx.handlers.onChooseStyle() },
    nodeCommand(CanvasNodeType.Config, { badge: () => canvasT("videoCanvas.toolbar.badgeHub", "枢纽"), defaultOrder: 8, run: (ctx) => ctx.handlers.onAddExtensionNode(CanvasNodeType.Config) }),
    nodeCommand(CanvasNodeType.Text, { defaultOrder: 10, run: (ctx) => ctx.handlers.onAddText() }),
    nodeCommand(CanvasNodeType.Drawing, { defaultOrder: 20, run: (ctx) => ctx.handlers.onAddDrawing() }),
    nodeCommand(CanvasNodeType.Script, { badge: () => canvasT("videoCanvas.toolbar.badgeCore", "核心"), defaultOrder: 30, run: (ctx) => ctx.handlers.onAddScript() }),
    nodeCommand(CanvasNodeType.Frame, { defaultOrder: 40, run: (ctx) => ctx.handlers.onAddFrame() }),
    { id: "folder", label: () => canvasT("videoCanvas.node.folder", "文件夹"), icon: <Folder />, badge: () => canvasT("videoCanvas.toolbar.badgeContainer", "容器"), section: "node", defaultOrder: 45, run: (ctx) => ctx.handlers.onAddFolder() },
    nodeCommand(CanvasNodeType.Image, { defaultOrder: 50, run: (ctx) => ctx.handlers.onAddImage() }),
    nodeCommand(CanvasNodeType.Video, { defaultOrder: 60, run: (ctx) => ctx.handlers.onAddVideo() }),
    { id: "director", label: () => canvasT("videoCanvas.toolbar.director", "导演台"), icon: <Layers3 />, badge: () => canvasT("videoCanvas.toolbar.badge3d", "3D"), section: "node", defaultOrder: 70, run: (ctx) => ctx.handlers.onOpenDirector() },
    nodeCommand(CanvasNodeType.Audio, { defaultOrder: 80, run: (ctx) => ctx.handlers.onAddAudio() }),
    // 展示与加工（扩展节点）
    extensionCommand(CanvasNodeType.Markdown, 10),
    extensionCommand(CanvasNodeType.Svg, 20),
    extensionCommand(CanvasNodeType.Html, 30),
    extensionCommand(CanvasNodeType.Panorama, 40),
    extensionCommand(CanvasNodeType.Compare, 50),
    extensionCommand(CanvasNodeType.Chart, 60),
    extensionCommand(CanvasNodeType.ColorGrade, 70),
    // 导入资源
    { id: "upload", label: () => canvasT("videoCanvas.toolbar.upload", "上传文件"), icon: <UploadCloud />, section: "resource", defaultOrder: 10, run: (ctx) => ctx.handlers.onUpload() },
    { id: "project-character", label: () => canvasT("videoCanvas.toolbar.addCharacter", "添加角色卡"), icon: <UserRound />, section: "resource", defaultOrder: 20, applicable: (ctx) => ctx.isProjectLinked, run: (ctx) => ctx.handlers.onOpenProjectCharacters() },
    { id: "assets", label: () => canvasT("videoCanvas.toolbar.assets", "素材库"), icon: <FolderOpen />, section: "resource", defaultOrder: 30, applicable: (ctx) => !ctx.isProjectLinked, run: (ctx) => ctx.handlers.onOpenMyAssets() },
];

registerAddNodeMenuCommands(addNodeMenuCommands);
