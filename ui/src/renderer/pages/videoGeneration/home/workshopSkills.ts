/**
 * Short-drama workshop catalog: genre directors first, then scene craft,
 * then storyboard manuals. Adjacent styles (ads / travel / MV) stay in the
 * builtin pack for canvas + search, but are hidden from the Featured tab
 * until the user searches.
 */

export type WorkshopSkillGroupId = "genre" | "scene" | "craft" | "adjacent";

export type WorkshopSkillGroup = {
  id: WorkshopSkillGroupId;
  labelKey: string;
  defaultLabel: string;
  names: readonly string[];
};

export const WORKSHOP_GENRE_SKILL_NAMES = [
  "short-drama",
  "female-drama",
  "revenge-rise",
  "costume-romance",
  "urban-ceo",
  "xianxia-romance",
  "horror-suspense",
] as const;

export const WORKSHOP_SCENE_SKILL_NAMES = ["scene-directing", "fight-fx"] as const;

export const WORKSHOP_CRAFT_SKILL_NAMES = [
  "character-bible",
  "scene-bible",
  "script-to-board",
  "replica-ref",
  "multi-shot-cut",
  "tail-to-head",
] as const;

export const WORKSHOP_ADJACENT_SKILL_NAMES = [
  "luxury-tvc",
  "wes-anderson",
  "product-demo",
  "travel-master",
  "music-visual",
  "documentary-observational",
] as const;

export const WORKSHOP_SKILL_GROUPS: readonly WorkshopSkillGroup[] = [
  {
    id: "genre",
    labelKey: "genre",
    defaultLabel: "题材导演",
    names: WORKSHOP_GENRE_SKILL_NAMES,
  },
  {
    id: "scene",
    labelKey: "scene",
    defaultLabel: "场面调度",
    names: WORKSHOP_SCENE_SKILL_NAMES,
  },
  {
    id: "craft",
    labelKey: "craft",
    defaultLabel: "制作手册",
    names: WORKSHOP_CRAFT_SKILL_NAMES,
  },
  {
    id: "adjacent",
    labelKey: "adjacent",
    defaultLabel: "其他风格",
    names: WORKSHOP_ADJACENT_SKILL_NAMES,
  },
];

export function skillBareName(id: string): string {
  const trimmed = id.trim();
  const colon = trimmed.indexOf(":");
  return colon >= 0 ? trimmed.slice(colon + 1) : trimmed;
}

export function workshopGroupForName(name: string): WorkshopSkillGroup | undefined {
  return WORKSHOP_SKILL_GROUPS.find((group) => group.names.includes(name));
}

export function isWorkshopFeaturedName(name: string): boolean {
  const group = workshopGroupForName(name);
  return Boolean(group && group.id !== "adjacent");
}

export function workshopRank(name: string): number {
  let offset = 0;
  for (const group of WORKSHOP_SKILL_GROUPS) {
    const index = group.names.indexOf(name);
    if (index >= 0) return offset + index;
    offset += group.names.length;
  }
  return offset + 100;
}
