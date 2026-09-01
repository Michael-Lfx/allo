import { canvasT } from "@oc/lib/canvas/canvas-i18n";

export const ASSET_CATEGORIES = ["character", "environment", "prop", "material", "other"] as const;

export type AssetCategory = (typeof ASSET_CATEGORIES)[number];

const ASSET_CATEGORY_DEFAULTS: Record<AssetCategory, string> = {
    character: "角色",
    environment: "场景",
    prop: "道具",
    material: "素材",
    other: "其他",
};

export const ASSET_CATEGORY_OPTIONS = ASSET_CATEGORIES.map((value) => ({
    value,
    get label() {
        return canvasT(`videoCanvas.asset.category.${value}`, ASSET_CATEGORY_DEFAULTS[value]);
    },
}));

const LEGACY_ASSET_CATEGORY_MAP: Record<string, AssetCategory> = {
    wardrobe: "prop",
    weapon: "prop",
    accessory: "prop",
    style: "material",
};

export function normalizeAssetCategory(value: unknown, fallback: AssetCategory = "other"): AssetCategory {
    if (typeof value !== "string") return fallback;
    const normalized = value.trim().toLowerCase();
    if ((ASSET_CATEGORIES as readonly string[]).includes(normalized)) return normalized as AssetCategory;
    return LEGACY_ASSET_CATEGORY_MAP[normalized] || fallback;
}

export function defaultAssetCategoryForKind(kind: string): AssetCategory {
    if (kind === "entity") return "character";
    if (kind === "image" || kind === "video" || kind === "audio" || kind === "model") return "material";
    return "other";
}

export function assetCategoryLabel(value: unknown) {
    const category = normalizeAssetCategory(value);
    return canvasT(`videoCanvas.asset.category.${category}`, ASSET_CATEGORY_DEFAULTS[category]);
}
