import { RECIPE_BY_ID } from "./recipes";

export const RECIPE_TOKEN_PATTERN = /@\[recipe:([^\]]+)\]/g;

export function recipeToken(id: string): string {
    return `@[recipe:${id}]`;
}

export function insertRecipeToken(prompt: string, recipeId: string): string {
    const token = recipeToken(recipeId);
    if (prompt.includes(token)) return prompt;
    const trimmed = prompt.replace(/(^|\s)\/[\p{L}\p{N}_-]*$/u, "$1").trimEnd();
    return trimmed ? `${trimmed} ${token}` : token;
}

export function removeRecipeToken(prompt: string, recipeId: string): string {
    return collapseSpaces(prompt.replace(new RegExp(`\\s*@\\[recipe:${escapeRegExp(recipeId)}\\]`, "g"), " "));
}

export function listedRecipeIds(prompt: string): string[] {
    const ids: string[] = [];
    const seen = new Set<string>();
    for (const match of prompt.matchAll(RECIPE_TOKEN_PATTERN)) {
        const id = match[1];
        if (!id || seen.has(id)) continue;
        seen.add(id);
        ids.push(id);
    }
    return ids;
}

export function stripRecipeTokens(prompt: string): string {
    return collapseSpaces(prompt.replace(RECIPE_TOKEN_PATTERN, " "));
}

export function mergeRecipeTokens(visible: string, recipeIds: string[]): string {
    const ids = uniqueIds([...listedRecipeIds(visible), ...recipeIds]);
    let next = stripRecipeTokens(visible);
    for (const id of ids) next = insertRecipeToken(next, id);
    return next;
}

export function expandRecipeTokens(prompt: string): string {
    if (!prompt.includes("@[recipe:")) return prompt;
    return prompt.replace(RECIPE_TOKEN_PATTERN, (token, id: string) => {
        const recipe = RECIPE_BY_ID.get(id);
        return recipe ? recipe.prompt : token;
    });
}

export type CraftTokenPart =
    | { type: "text"; value: string }
    | { type: "recipe"; id: string }
    | { type: "skill"; id: string };

const CRAFT_TOKEN_PATTERN = /@\[(recipe|skill):([^\]]+)\]/g;

export function splitCraftTokenParts(value: string): CraftTokenPart[] {
    const parts: CraftTokenPart[] = [];
    let lastIndex = 0;
    for (const match of value.matchAll(CRAFT_TOKEN_PATTERN)) {
        if (match.index === undefined) continue;
        if (match.index > lastIndex) parts.push({ type: "text", value: value.slice(lastIndex, match.index) });
        parts.push(match[1] === "recipe" ? { type: "recipe", id: match[2] } : { type: "skill", id: match[2] });
        lastIndex = match.index + match[0].length;
    }
    if (lastIndex < value.length) parts.push({ type: "text", value: value.slice(lastIndex) });
    return parts;
}

export function uniqueIds(ids: string[]): string[] {
    const seen = new Set<string>();
    const out: string[] = [];
    for (const id of ids) {
        if (!id || seen.has(id)) continue;
        seen.add(id);
        out.push(id);
    }
    return out;
}

function collapseSpaces(value: string): string {
    return value.replace(/\s{2,}/g, " ").trim();
}

function escapeRegExp(value: string): string {
    return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
