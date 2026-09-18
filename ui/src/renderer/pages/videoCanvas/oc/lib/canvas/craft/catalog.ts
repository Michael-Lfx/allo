import type { CanvasGenerationMode, CanvasNodeData } from "@oc/types/canvas";

import { insertSkillToken, listedSkillIds, stripSkillTokens } from "../canvas-skill-mentions";
import { BUILTIN_PLAYBOOKS, PLAYBOOK_BY_QUALIFIED } from "./playbooks";
import { CRAFT_RECIPES, RECIPE_BY_ID } from "./recipes";
import { insertRecipeToken, listedRecipeIds, stripRecipeTokens, uniqueIds } from "./tokens";
import type { CraftPlaybook, CraftRecipe } from "./types";

export { CRAFT_GRAPHS, GRAPH_BY_ID } from "./graphs";
export { BUILTIN_PLAYBOOKS, PLAYBOOK_BY_ID, PLAYBOOK_BY_QUALIFIED, PLAYBOOK_CATEGORIES } from "./playbooks";
export { CRAFT_RECIPES, RECIPE_BY_ID, RECIPE_GROUPS } from "./recipes";
export { craftCover, craftStillUrl } from "./covers";
export type { PlaybookCategory } from "./playbooks";
export { craftText } from "./locale";
export { expandRecipeTokens, insertRecipeToken, listedRecipeIds, mergeRecipeTokens, recipeToken, removeRecipeToken, splitCraftTokenParts, stripRecipeTokens } from "./tokens";
export { buildGraphStarter, buildPlaybookNode, playbookSnapshot, recipeApplyPatch } from "./apply";
export type { CraftGraph, CraftKind, CraftPlaybook, CraftRecipe, LibraryTab, RecipeGroup, RecipeInput } from "./types";
export { getAgentPlaybook, listAgentPlaybooks } from "./agent-catalog";

export type CraftMediaKind = "image" | "video" | "both";
export type CraftMediaScope = "all" | "imageOnly" | "videoOnly" | "both";

export function recipeMediaKind(item: CraftRecipe): CraftMediaKind {
    const hasImage = item.modes.includes("image");
    const hasVideo = item.modes.includes("video");
    if (hasImage && hasVideo) return "both";
    if (hasVideo) return "video";
    if (hasImage) return "image";
    return "both";
}

export function recipeFitsMedia(item: CraftRecipe, scope: CraftMediaScope): boolean {
    if (scope === "all") return true;
    if (scope === "imageOnly") return recipeMediaKind(item) === "image";
    if (scope === "videoOnly") return recipeMediaKind(item) === "video";
    return recipeMediaKind(item) === "both";
}

export function recipesForMode(mode: CanvasGenerationMode, query = ""): CraftRecipe[] {
    const needle = query.trim().toLowerCase();
    return CRAFT_RECIPES.filter((item) => {
        if (!item.modes.includes(mode)) return false;
        if (!needle) return true;
        return `${item.id} ${item.title.zh} ${item.title.en} ${item.job.zh} ${item.job.en}`.toLowerCase().includes(needle);
    });
}

export function findPlaybook(idOrQualified: string): CraftPlaybook | undefined {
    return PLAYBOOK_BY_QUALIFIED.get(idOrQualified) || PLAYBOOK_BY_QUALIFIED.get(`builtin:${idOrQualified}`) || BUILTIN_PLAYBOOKS.find((item) => item.id === idOrQualified || item.title.zh === idOrQualified || item.title.en.toLowerCase() === idOrQualified.toLowerCase());
}

export function findRecipe(id: string): CraftRecipe | undefined {
    return RECIPE_BY_ID.get(id);
}

export function mergeCraftAttachments(visible: string, recipeIds: string[], skillIds: string[]): string {
    const recipes = uniqueIds([...listedRecipeIds(visible), ...recipeIds]);
    const skills = uniqueIds([...listedSkillIds(visible), ...skillIds]);
    let next = stripSkillTokens(stripRecipeTokens(visible));
    for (const id of recipes) next = insertRecipeToken(next, id);
    for (const id of skills) next = insertSkillToken(next, id);
    return next;
}

/** Project playbook qualified id (`builtin:…` / `hub:…`) when a Skill node is marked projectPlaybook. */
export function canvasPlaybookId(nodes: Pick<CanvasNodeData, "metadata">[]): string | undefined {
    const id = nodes.find((node) => node.metadata?.projectPlaybook)?.metadata?.skillId?.trim();
    return id || undefined;
}

/** Accepts a already-qualified id, or resolves a builtin short id. Looks are not playbooks. */
export function qualifiedPlaybookId(raw?: string | null): string | undefined {
    const value = (raw || "").trim();
    if (!value) return undefined;
    if (value.startsWith("builtin:") || value.startsWith("hub:")) return value;
    return findPlaybook(value)?.qualifiedId;
}
