import { scopedLocalStorage } from "@oc/lib/user-scope";

const KEY = "canvas-craft-recents-v1";
const LIMIT = 8;

export function readCraftRecents(): string[] {
    try {
        const raw = scopedLocalStorage.getItem(KEY);
        if (!raw) return [];
        const parsed = JSON.parse(raw) as unknown;
        return Array.isArray(parsed) ? parsed.filter((item): item is string => typeof item === "string").slice(0, LIMIT) : [];
    } catch {
        return [];
    }
}

export function rememberCraftRecent(id: string) {
    const next = [id, ...readCraftRecents().filter((item) => item !== id)].slice(0, LIMIT);
    try {
        scopedLocalStorage.setItem(KEY, JSON.stringify(next));
    } catch {
        // ignore quota
    }
}
