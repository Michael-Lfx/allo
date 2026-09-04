import data from "../../content/market.json";

import type { Language } from "../i18n";

export interface MarketExpert {
  name: string;
  description: string;
  avatar: string | null;
}

export interface MarketSkill {
  name: string;
  version: string;
  source: string;
  description_zh: string;
  description_en: string;
  tags_zh: string[];
  tags_en: string[];
  avatar: string | null;
}

export interface MarketConnector {
  id: string;
  name: string;
  name_en: string;
  description_zh: string;
  description_en: string;
  description: string;
  avatar: string | null;
}

export interface MarketSnapshot {
  updatedAt: string;
  base: string;
  experts: MarketExpert[];
  skills: MarketSkill[];
  connectors: MarketConnector[];
}

export type MarketTab = "experts" | "skills" | "connectors";

export const MARKET_TABS: MarketTab[] = ["experts", "skills", "connectors"];

export const market: MarketSnapshot = data as MarketSnapshot;

export const marketCounts: Record<MarketTab, number> = {
  experts: market.experts.length,
  skills: market.skills.length,
  connectors: market.connectors.length,
};

export const marketTotal = market.experts.length + market.skills.length + market.connectors.length;

/** Deterministic pastel hue for letter-badge fallbacks. */
export function nameHue(name: string): number {
  let h = 0;
  for (let i = 0; i < name.length; i++) h = (h * 31 + name.charCodeAt(i)) % 360;
  return h;
}

/** Site-absolute URL for a snapshot avatar path (respects the base path). */
export function avatarUrl(avatar: string): string {
  if (/^https?:\/\//i.test(avatar)) return avatar;
  const base = import.meta.env.BASE_URL || "/";
  return `${base.replace(/\/*$/, "/")}${avatar.replace(/^\/+/, "")}`;
}

/** Localized display name for an entry. */
export function entryName(
  entry: MarketExpert | MarketSkill | MarketConnector,
  lang: Language,
): string {
  if ("name_en" in entry && lang === "en-US" && entry.name_en) return entry.name_en;
  return entry.name;
}

/** Localized description for an entry (falls back to whatever exists). */
export function entryDescription(
  entry: MarketExpert | MarketSkill | MarketConnector,
  lang: Language,
): string {
  if ("description_zh" in entry || "description_en" in entry) {
    const e = entry as MarketSkill | MarketConnector;
    return lang === "en-US" ? e.description_en || e.description_zh : e.description_zh || e.description_en;
  }
  return (entry as MarketExpert).description;
}

/** Localized tags for skills (experts/connectors carry no tags). */
export function entryTags(entry: MarketExpert | MarketSkill | MarketConnector, lang: Language): string[] {
  if ("tags_zh" in entry) {
    const e = entry as MarketSkill;
    return lang === "en-US" ? e.tags_en : e.tags_zh;
  }
  return [];
}

/** Case-insensitive match over name + description + tags. */
export function entryMatches(
  entry: MarketExpert | MarketSkill | MarketConnector,
  lang: Language,
  query: string,
): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  const hay = [entryName(entry, lang), entryDescription(entry, lang), ...entryTags(entry, lang)]
    .join("\n")
    .toLowerCase();
  return q.split(/\s+/).every((part) => hay.includes(part));
}
