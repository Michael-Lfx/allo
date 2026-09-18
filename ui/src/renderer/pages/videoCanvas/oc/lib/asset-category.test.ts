import { describe, expect, test } from "bun:test";

import { defaultAssetCategoryForKind, normalizeAssetCategory } from "./asset-category";

describe("asset category contract", () => {
    test("migrates wardrobe, weapon, and accessory into prop", () => {
        expect(normalizeAssetCategory("wardrobe")).toBe("prop");
        expect(normalizeAssetCategory("weapon")).toBe("prop");
        expect(normalizeAssetCategory("accessory")).toBe("prop");
    });

    test("migrates style into material and unknown values into other", () => {
        expect(normalizeAssetCategory("style")).toBe("material");
        expect(normalizeAssetCategory("unknown")).toBe("other");
    });

    test("defaults uncategorized media to material", () => {
        expect(defaultAssetCategoryForKind("image")).toBe("material");
        expect(defaultAssetCategoryForKind("video")).toBe("material");
        expect(defaultAssetCategoryForKind("audio")).toBe("material");
        expect(defaultAssetCategoryForKind("text")).toBe("other");
    });
});
