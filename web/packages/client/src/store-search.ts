/**
 * Catalogue search: one field table, three consumers (match, rank, highlight).
 *
 * `StoreClient.search()` used to build a private haystack string and scan it,
 * which made the matching rule unshareable: a caller that wanted to *highlight*
 * a hit had to re-implement the field list and the case folding, and any drift
 * between the two copies produced "matched but not highlighted" (or the
 * reverse). It also joined fields with `\n`, so a query with a space could never
 * match across them.
 *
 * Everything searchable is declared once in {@link SEARCHABLE_FIELDS}; the
 * haystack, the relevance rank and the highlight ranges are all derived from it,
 * so adding a field is a one-line change that every consumer sees.
 *
 * Two invariants this module keeps:
 *
 * - **Ranges point at the original text.** Folding is only ever used to *find*
 *   a match; a value whose folded form changes length (e.g. `"İ"` folds to two
 *   code units) reports the hit without ranges rather than reporting offsets
 *   into a string nobody renders.
 * - **Nothing here invents text.** The prompt examples a market declares are
 *   display data (see doc `49` §3.7), never search input.
 */

import type { StoreItem, StoreItemKind } from "@flowy-agent-store/protocol";

/** A searchable field of `StoreItem` (doc `49` §3.1, INV-1). */
export type StoreSearchField =
  | "entry_name"
  | "name"
  | "id"
  | "version"
  | "description"
  | "display_name"
  | "display_description"
  | "profession"
  | "tags"
  | "category";

/**
 * Relevance bucket, lowest wins: `1` = the name itself, `2` = a semantic key
 * (tag / category / profession), `3` = long free text. Deliberately three
 * buckets and not a score: the catalogue is ~900 entries, and a caller cannot
 * debug a number it did not compute.
 */
export type StoreSearchWeight = 1 | 2 | 3;

/** One string a field carries, plus which language it came from. */
interface FieldValue {
  text: string;
  lang?: "zh" | "en";
}

interface FieldSource {
  field: StoreSearchField;
  weight: StoreSearchWeight;
  values(item: StoreItem): FieldValue[];
}

function localizedValues(text: { zh?: string | null; en?: string | null } | null): FieldValue[] {
  if (!text) return [];
  const out: FieldValue[] = [];
  if (text.zh) out.push({ text: text.zh, lang: "zh" });
  if (text.en) out.push({ text: text.en, lang: "en" });
  return out;
}

/**
 * Every field a user can see, in the order the haystack used to concatenate
 * them. `examples_*` / `quick_prompts` are **not** here on purpose (doc `49`
 * D9): example prompts are long free text and would drown real name hits.
 */
export const SEARCHABLE_FIELDS: readonly FieldSource[] = [
  { field: "entry_name", weight: 1, values: (item) => [{ text: item.entry_name }] },
  { field: "name", weight: 1, values: (item) => [{ text: item.name }] },
  { field: "id", weight: 1, values: (item) => [{ text: item.id }] },
  { field: "version", weight: 3, values: (item) => [{ text: item.version }] },
  {
    field: "description",
    weight: 3,
    values: (item) => (item.description ? [{ text: item.description }] : []),
  },
  { field: "display_name", weight: 1, values: (item) => localizedValues(item.display_name ?? null) },
  {
    field: "display_description",
    weight: 3,
    values: (item) => localizedValues(item.display_description ?? null),
  },
  { field: "profession", weight: 2, values: (item) => localizedValues(item.profession ?? null) },
  {
    field: "tags",
    weight: 2,
    values: (item) => (item.tags ?? []).flatMap((tag) => localizedValues(tag)),
  },
  { field: "category", weight: 2, values: (item) => localizedValues(item.category?.name ?? null) },
];

/** The flat text a query is matched against — same field set, same order. */
export function storeHaystack(item: StoreItem): string {
  return SEARCHABLE_FIELDS.flatMap((source) => source.values(item).map((value) => value.text))
    .join("\n")
    .toLowerCase();
}

/**
 * Split a query into terms on whitespace, lower-cased.
 *
 * Every term must be found somewhere in the entry (AND, each term independently
 * — doc `49` D2). A single-term query therefore behaves exactly as the old
 * `haystack().includes()` did.
 */
export function splitStoreQuery(query: string): string[] {
  return query.trim().toLowerCase().split(/\s+/).filter(Boolean);
}

/** One field's highlights: `ranges` are `[start, end)` on the **original** text. */
export interface StoreFieldHit {
  field: StoreSearchField;
  /** Present only on localized fields: which language matched. */
  lang?: "zh" | "en";
  /** Sorted, non-overlapping. */
  ranges: Array<[number, number]>;
}

export interface StoreMatch {
  item: StoreItem;
  hits: StoreFieldHit[];
  /**
   * `true` when every hit landed on a localized field whose language is not the
   * caller's UI language — the rendered label will not visibly contain the
   * query. A hit on a language-neutral field (`name`, `description`, `id`, …)
   * is always visible, so it forces this to `false`.
   */
  offLanguage: boolean;
}

export interface StoreMatchOptions {
  /**
   * The UI language, used only for {@link StoreMatch.offLanguage}. Defaults to
   * `zh` because that is this host's default UI language; a caller rendering
   * another language should pass its own.
   */
  lang?: "zh" | "en";
}

function occurrences(haystack: string, needle: string): Array<[number, number]> {
  const out: Array<[number, number]> = [];
  let from = 0;
  for (;;) {
    const at = haystack.indexOf(needle, from);
    if (at < 0) return out;
    out.push([at, at + needle.length]);
    from = at + needle.length;
  }
}

function mergeRanges(ranges: Array<[number, number]>): Array<[number, number]> {
  if (ranges.length < 2) return ranges;
  const sorted = [...ranges].sort((a, b) => a[0] - b[0] || a[1] - b[1]);
  const merged: Array<[number, number]> = [sorted[0]];
  for (const range of sorted.slice(1)) {
    const last = merged[merged.length - 1];
    if (range[0] <= last[1]) {
      last[1] = Math.max(last[1], range[1]);
    } else {
      merged.push(range);
    }
  }
  return merged;
}

/**
 * Match one entry against a query.
 *
 * Returns `null` when any term is missing (the entry is not a hit), and a match
 * with an empty `hits` array for an empty query (the entry passes the text
 * filter but has nothing to highlight).
 */
export function matchStoreItem(
  item: StoreItem,
  query: string,
  options: StoreMatchOptions = {},
): StoreMatch | null {
  const terms = splitStoreQuery(query);
  if (terms.length === 0) {
    return { item, hits: [], offLanguage: false };
  }
  const uiLang = options.lang ?? "zh";
  const found = new Set<string>();
  const hits: StoreFieldHit[] = [];
  let neutralHit = false;

  for (const source of SEARCHABLE_FIELDS) {
    for (const value of source.values(item)) {
      const folded = value.text.toLowerCase();
      // Folding changed the length, so folded offsets no longer address the
      // original string. Keep the hit, drop the ranges (doc `49` D4).
      const rangeable = folded.length === value.text.length;
      const ranges: Array<[number, number]> = [];
      for (const term of terms) {
        const at = occurrences(folded, term);
        if (at.length === 0) continue;
        found.add(term);
        if (!rangeable) continue;
        ranges.push(...at);
        if (!value.lang) neutralHit = true;
      }
      if (ranges.length > 0) {
        hits.push(
          value.lang
            ? { field: source.field, lang: value.lang, ranges: mergeRanges(ranges) }
            : { field: source.field, ranges: mergeRanges(ranges) },
        );
      }
    }
  }

  if (terms.some((term) => !found.has(term))) return null;

  hits.sort((a, b) => a.field.localeCompare(b.field) || (a.ranges[0]?.[0] ?? 0) - (b.ranges[0]?.[0] ?? 0));
  const offLanguage = !neutralHit && hits.length > 0 && hits.every((hit) => hit.lang !== undefined && hit.lang !== uiLang);
  return { item, hits, offLanguage };
}

/** The best (lowest) relevance bucket among a match's hit fields. */
export function storeMatchRank(match: StoreMatch): StoreSearchWeight {
  let rank: StoreSearchWeight = 3;
  for (const hit of match.hits) {
    const source = SEARCHABLE_FIELDS.find((candidate) => candidate.field === hit.field);
    if (source && source.weight < rank) rank = source.weight;
  }
  return rank;
}

/** Options accepted by `searchWithHits()` / `searchInstalled()` (doc `49` §3.4). */
export interface StoreSearchOptions extends StoreMatchOptions {
  kind?: StoreItemKind;
  /**
   * Pre-fetched catalogue. Supplying it makes the call **zero-I/O**: the caller
   * keeps one array from `listStatus()` and filters it on every keystroke
   * (doc `49` D6 — prefetch instead of a cache nobody can invalidate correctly).
   */
  items?: StoreItem[];
  /** Keep only installed entries. */
  installedOnly?: boolean;
}
