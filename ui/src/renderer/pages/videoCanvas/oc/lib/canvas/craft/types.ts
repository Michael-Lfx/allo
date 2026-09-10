import type { CanvasGenerationMode, CanvasNodeData, CanvasConnection, Position } from "@oc/types/canvas";

export type LibraryTab = "recipe" | "playbook" | "graph" | "community";
export type CraftKind = "recipe" | "playbook" | "graph";
export type RecipeGroup = "character" | "coverage" | "continuity" | "lighting" | "camera" | "craft";
export type RecipeInput = "text" | "one-image" | "current" | "script" | "two-people";

export type LocaleText = { zh: string; en: string };

export type CraftRecipe = {
    kind: "recipe";
    id: string;
    group: RecipeGroup;
    title: LocaleText;
    job: LocaleText;
    prompt: string;
    modes: CanvasGenerationMode[];
    input: RecipeInput;
    /** Still id under `ui/public/craft/{id}.png` (same as recipe id). */
    coverLookId: string;
    cameraMoveId?: string;
};

export type CraftPlaybook = {
    kind: "playbook";
    id: string;
    qualifiedId: string;
    title: LocaleText;
    job: LocaleText;
    category: string;
    coverLookId: string;
    brief: string;
    origin: "builtin" | "hub";
};

export type CraftGraph = {
    kind: "graph";
    id: string;
    title: LocaleText;
    job: LocaleText;
    coverLookId: string;
};

export type CraftGraphBuild = {
    nodes: CanvasNodeData[];
    connections: CanvasConnection[];
};

export type RecipeApplyPatch = {
    prompt: string;
    cameraMoveId?: string;
    cameraMovePrompt?: string;
};

export type PlaybookSnapshotInput = {
    playbook: CraftPlaybook;
    template: string;
    coverUrl?: string;
    position: Position;
};
