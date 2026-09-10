import type { CSSProperties, ReactElement, ReactNode } from "react";
import { cloneElement, isValidElement } from "react";
import { useTranslation } from "react-i18next";

import { canvasT } from "@oc/lib/canvas/canvas-i18n";
import { canvasThemes } from "@oc/lib/canvas-theme";
import { cn } from "@oc/lib/utils";
import { useThemeStore } from "@oc/stores/use-theme-store";

export type CanvasCreateCommand = {
    id: string;
    label: string;
    icon: ReactNode;
    badge?: string;
    section: "node" | "extension" | "project" | "resource";
    onClick: () => void;
};

export function CanvasCreateMenu({ commands, compactCreateMenu = false }: { commands: CanvasCreateCommand[]; compactCreateMenu?: boolean }) {
    useTranslation();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const projectCommands = commands.filter((command) => command.section === "project");
    const nodeCommands = commands.filter((command) => command.section === "node");
    const extensionCommands = commands.filter((command) => command.section === "extension");
    const resourceCommands = commands.filter((command) => command.section === "resource");
    const extensionTitle = compactCreateMenu
        ? canvasT("videoCanvas.menu.moreNodes", "更多")
        : canvasT("videoCanvas.menu.extensionNodes", "展示与加工");

    return (
        <div>
            {nodeCommands.length ? (
                <>
                    <MenuSection title={canvasT("videoCanvas.menu.createNodes", "创作节点")} color={theme.node.muted} />
                    <CanvasCreateCommandGrid commands={nodeCommands} variant="node" />
                </>
            ) : null}

            {projectCommands.length ? (
                <>
                    <MenuSection title={canvasT("videoCanvas.menu.workbench", "工作台")} color={theme.node.muted} spaced={Boolean(nodeCommands.length)} />
                    <CanvasCreateCommandGrid commands={projectCommands} variant="resource" />
                </>
            ) : null}

            {extensionCommands.length ? (
                <>
                    <MenuSection title={extensionTitle} color={theme.node.muted} spaced />
                    <CanvasCreateCommandGrid commands={extensionCommands} variant="node" />
                </>
            ) : null}

            {resourceCommands.length ? (
                <>
                    <MenuSection title={canvasT("videoCanvas.menu.importResources", "导入资源")} color={theme.node.muted} spaced />
                    <CanvasCreateCommandGrid commands={resourceCommands} variant="resource" />
                </>
            ) : null}
        </div>
    );
}

function CanvasCreateCommandGrid({ commands, variant }: { commands: CanvasCreateCommand[]; variant: "node" | "resource" }) {
    const theme = canvasThemes[useThemeStore((state) => state.theme)];

    return (
        <div className={cn("grid gap-0.5", variant === "node" ? "grid-cols-4" : "grid-cols-2")}>
            {commands.map((command) => (
                <button
                    key={command.id}
                    type="button"
                    className={cn(
                        "group min-w-0 overflow-hidden outline-none hover:bg-black/5 focus-visible:ring-2 dark:hover:bg-white/8",
                        variant === "node"
                            ? "flex h-[var(--canvas-create-node-height)] flex-col items-start justify-between rounded-md px-1 py-1.5 text-left"
                            : "flex h-[var(--canvas-create-resource-height)] items-center justify-center gap-1.5 rounded-md px-2 text-center",
                    )}
                    style={{ color: theme.node.text, "--tw-ring-color": theme.node.muted } as CSSProperties}
                    title={command.label}
                    onMouseDown={(event) => event.stopPropagation()}
                    onClick={command.onClick}
                >
                    {variant === "node" ? (
                        <>
                            <span className="flex w-full min-w-0 items-center justify-between gap-1">
                                <CreateCommandIcon icon={command.icon} />
                                {command.badge ? <span className="shrink-0 font-medium leading-none" style={{ color: theme.node.muted, fontSize: "var(--fs-label)" }}>{command.badge}</span> : null}
                            </span>
                            <span className="block w-full overflow-hidden text-ellipsis whitespace-nowrap font-medium leading-none" style={{ fontSize: "var(--fs-label)" }}>{command.label}</span>
                        </>
                    ) : (
                        <>
                            <CreateCommandIcon icon={command.icon} resource />
                            <span className="min-w-0 overflow-hidden text-ellipsis whitespace-nowrap font-medium leading-none" style={{ fontSize: "var(--fs-label)" }}>{command.label}</span>
                        </>
                    )}
                </button>
            ))}
        </div>
    );
}

function CreateCommandIcon({ icon, resource = false }: { icon: ReactNode; resource?: boolean }) {
    const glyph = isValidElement(icon)
        ? cloneElement(icon as ReactElement<{ size?: number; strokeWidth?: number }>, { size: resource ? 14 : 16, strokeWidth: 1.75 })
        : icon;
    return <span className={cn("canvas-create-command-icon", resource && "is-resource")}>{glyph}</span>;
}

function MenuSection({ title, color, spaced = false }: { title: string; color: string; spaced?: boolean }) {
    return <h3 className="mb-1 mt-2 px-1 font-medium leading-none" style={{ color, fontSize: "var(--fs-label)", marginTop: spaced ? "var(--space-3)" : "var(--space-2)" }}>{title}</h3>;
}
