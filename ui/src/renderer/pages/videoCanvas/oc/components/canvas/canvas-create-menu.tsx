import type { CSSProperties, ReactNode } from "react";
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

export function CanvasCreateMenu({ commands }: { commands: CanvasCreateCommand[] }) {
    useTranslation();
    const theme = canvasThemes[useThemeStore((state) => state.theme)];
    const projectCommands = commands.filter((command) => command.section === "project");
    const nodeCommands = commands.filter((command) => command.section === "node");
    const extensionCommands = commands.filter((command) => command.section === "extension");
    const resourceCommands = commands.filter((command) => command.section === "resource");

    return (
        <div>
            {projectCommands.length ? (
                <div className="mb-1.5 flex flex-wrap justify-end gap-0.5">
                    {projectCommands.map((command) => (
                        <button
                            key={command.id}
                            type="button"
                            className="inline-flex h-6 items-center gap-1 rounded-md px-1.5 font-medium outline-none hover:bg-black/5 focus-visible:ring-2 dark:hover:bg-white/8 [&_svg]:size-3"
                            style={{ color: theme.node.muted, fontSize: "var(--fs-label)", "--tw-ring-color": theme.node.muted } as CSSProperties}
                            title={command.label}
                            onMouseDown={(event) => event.stopPropagation()}
                            onClick={command.onClick}
                        >
                            {command.icon}
                            <span className="whitespace-nowrap">{command.label}</span>
                        </button>
                    ))}
                </div>
            ) : null}

            <MenuSection title={canvasT("videoCanvas.menu.createNodes", "创作节点")} color={theme.node.muted} />
            <CanvasCreateCommandGrid commands={nodeCommands} variant="node" />

            {extensionCommands.length ? (
                <>
                    <MenuSection title={canvasT("videoCanvas.menu.extensionNodes", "展示与加工")} color={theme.node.muted} spaced />
                    <CanvasCreateCommandGrid commands={extensionCommands} variant="node" />
                </>
            ) : null}

            <MenuSection title={canvasT("videoCanvas.menu.importResources", "导入资源")} color={theme.node.muted} spaced />
            <CanvasCreateCommandGrid commands={resourceCommands} variant="resource" />
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
                                <span className="grid size-4 shrink-0 place-items-center opacity-60 group-hover:opacity-100 [&_svg]:size-4">{command.icon}</span>
                                {command.badge ? <span className="shrink-0 font-medium leading-none" style={{ color: theme.node.muted, fontSize: "var(--fs-label)" }}>{command.badge}</span> : null}
                            </span>
                            <span className="block w-full overflow-hidden text-ellipsis whitespace-nowrap font-medium leading-none" style={{ fontSize: "var(--fs-label)" }}>{command.label}</span>
                        </>
                    ) : (
                        <>
                            <span className="grid size-4 shrink-0 place-items-center opacity-60 group-hover:opacity-100 [&_svg]:size-3.5">{command.icon}</span>
                            <span className="min-w-0 overflow-hidden text-ellipsis whitespace-nowrap font-medium leading-none" style={{ fontSize: "var(--fs-label)" }}>{command.label}</span>
                        </>
                    )}
                </button>
            ))}
        </div>
    );
}

function MenuSection({ title, color, spaced = false }: { title: string; color: string; spaced?: boolean }) {
    return <h3 className="mb-1 mt-2 px-1 font-medium leading-none" style={{ color, fontSize: "var(--fs-label)", marginTop: spaced ? "var(--space-3)" : "var(--space-2)" }}>{title}</h3>;
}
