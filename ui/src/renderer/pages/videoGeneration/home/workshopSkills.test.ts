import { describe, expect, test } from "bun:test";

import {
  isWorkshopFeaturedName,
  skillBareName,
  workshopRank,
  WORKSHOP_ADJACENT_SKILL_NAMES,
  WORKSHOP_GENRE_SKILL_NAMES,
} from "./workshopSkills";

describe("workshop skill catalog", () => {
  test("featured tab keeps short-drama genres and hides ads until search", () => {
    expect(WORKSHOP_GENRE_SKILL_NAMES).toContain("revenge-rise");
    expect(WORKSHOP_GENRE_SKILL_NAMES).toContain("costume-romance");
    expect(isWorkshopFeaturedName("short-drama")).toBe(true);
    expect(isWorkshopFeaturedName("scene-directing")).toBe(true);
    expect(isWorkshopFeaturedName("character-bible")).toBe(true);
    for (const name of WORKSHOP_ADJACENT_SKILL_NAMES) {
      expect(isWorkshopFeaturedName(name)).toBe(false);
    }
  });

  test("ranks genre directors before adjacent commercial styles", () => {
    expect(skillBareName("builtin:urban-ceo")).toBe("urban-ceo");
    expect(workshopRank("short-drama")).toBeLessThan(workshopRank("female-drama"));
    expect(workshopRank("female-drama")).toBeLessThan(workshopRank("luxury-tvc"));
    expect(workshopRank("tail-to-head")).toBeLessThan(workshopRank("product-demo"));
  });
});
