import { BookOpen, Folder, Layers3, Library, Palette, UploadCloud, UserRound } from "lucide-react";
import type { ReactNode } from "react";

import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { getNodeLabel } from "@oc/lib/canvas/node-registry";
import { BUILTIN_NODE_ICONS } from "@oc/lib/canvas/node-registry/definitions/builtin-nodes";
import { CanvasNodeType } from "@oc/types/canvas";

import { registerAddNodeMenuCommands } from "../tool-registry";
import type { AddNodeMenuCommand } from "../tool-definition";

/** 真正创建节点的命令：文案走 i18n，图标用节点类型对应的 Lucide 组件。 */
function nodeCommand(type: CanvasNodeType, rest: Omit<AddNodeMenuCommand, "id" | "label" | "icon">): AddNodeMenuCommand {
    return { id: type, label: () => getNodeLabel(type), icon: () => nodeTypeIcon(type), section: "node", ...rest };
}

/**
 * 扩展节点命令：落在独立的「展示与加工」分区，不挤占上方调好的四列创作网格。
 */
function extensionCommand(type: CanvasNodeType, defaultOrder: number): AddNodeMenuCommand {
    return {
        id: type,
        label: () => getNodeLabel(type),
        icon: () => nodeTypeIcon(type),
        section: "extension",
        defaultOrder,
        run: (ctx) => ctx.handlers.onAddExtensionNode(type),
    };
}

function nodeTypeIcon(type: CanvasNodeType): ReactNode {
    const Icon = BUILTIN_NODE_ICONS[type];
    return <Icon size={16} strokeWidth={1.75} />;
}

export const addNodeMenuCommands: AddNodeMenuCommand[] = [
    // 工作台：打开面板/货架，不是往画布丢节点。
    { id: "style", label: () => canvasT("videoCanvas.toolbar.style", "项目画风"), icon: <Palette />, badge: () => canvasT("videoCanvas.toolbar.badgeBaseline", "基线"), section: "project", defaultOrder: 5, applicable: (ctx) => !ctx.isProjectLinked, run: (ctx) => ctx.handlers.onChooseStyle() },
    { id: "library", label: () => canvasT("videoCanvas.toolbar.library", "货架"), icon: <BookOpen />, badge: () => canvasT("videoCanvas.toolbar.badgeCraft", "手法"), section: "project", defaultOrder: 6, run: (ctx) => ctx.handlers.onOpenLibrary() },
    { id: "director", label: () => canvasT("videoCanvas.toolbar.director", "导演台"), icon: <Layers3 />, badge: () => canvasT("videoCanvas.toolbar.badge3d", "3D"), section: "project", defaultOrder: 7, run: (ctx) => ctx.handlers.onOpenDirector() },
    // 创作节点只放会落到画布上的片原语。
    nodeCommand(CanvasNodeType.Script, { badge: () => canvasT("videoCanvas.toolbar.badgeCore", "核心"), defaultOrder: 10, run: (ctx) => ctx.handlers.onAddScript() }),
    nodeCommand(CanvasNodeType.Image, { defaultOrder: 20, run: (ctx) => ctx.handlers.onAddImage() }),
    nodeCommand(CanvasNodeType.Video, { defaultOrder: 30, run: (ctx) => ctx.handlers.onAddVideo() }),
    nodeCommand(CanvasNodeType.Audio, { defaultOrder: 50, run: (ctx) => ctx.handlers.onAddAudio() }),
    // 展示与加工：配置/笔记/容器 + 扩展节点
    nodeCommand(CanvasNodeType.Config, { section: "extension", badge: () => canvasT("videoCanvas.toolbar.badgeHub", "枢纽"), defaultOrder: 8, run: (ctx) => ctx.handlers.onAddExtensionNode(CanvasNodeType.Config) }),
    nodeCommand(CanvasNodeType.Text, { section: "extension", defaultOrder: 12, run: (ctx) => ctx.handlers.onAddText() }),
    nodeCommand(CanvasNodeType.Drawing, { section: "extension", defaultOrder: 16, run: (ctx) => ctx.handlers.onAddDrawing() }),
    nodeCommand(CanvasNodeType.Frame, { section: "extension", defaultOrder: 18, run: (ctx) => ctx.handlers.onAddFrame() }),
    { id: "folder", label: () => canvasT("videoCanvas.node.folder", "文件夹"), icon: <Folder />, badge: () => canvasT("videoCanvas.toolbar.badgeContainer", "容器"), section: "extension", defaultOrder: 19, run: (ctx) => ctx.handlers.onAddFolder() },
    extensionCommand(CanvasNodeType.Markdown, 20),
    extensionCommand(CanvasNodeType.Svg, 30),
    extensionCommand(CanvasNodeType.Html, 40),
    extensionCommand(CanvasNodeType.Panorama, 50),
    extensionCommand(CanvasNodeType.Compare, 60),
    extensionCommand(CanvasNodeType.Chart, 70),
    extensionCommand(CanvasNodeType.ColorGrade, 80),
    extensionCommand(CanvasNodeType.ArtCritique, 90),
    // 导入资源
    { id: "upload", label: () => canvasT("videoCanvas.toolbar.upload", "上传文件"), icon: <UploadCloud />, section: "resource", defaultOrder: 10, run: (ctx) => ctx.handlers.onUpload() },
    { id: "project-character", label: () => canvasT("videoCanvas.toolbar.addCharacter", "添加角色卡"), icon: <UserRound />, section: "resource", defaultOrder: 20, applicable: (ctx) => ctx.isProjectLinked, run: (ctx) => ctx.handlers.onOpenProjectCharacters() },
    { id: "assets", label: () => canvasT("videoCanvas.toolbar.assets", "素材空间"), icon: <Library />, section: "resource", defaultOrder: 30, run: (ctx) => ctx.handlers.onOpenMyAssets() },
];

registerAddNodeMenuCommands(addNodeMenuCommands);
