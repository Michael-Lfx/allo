/**
 * Ordering rules for the catalog's store projection.
 *
 * Pure and dependency-free on purpose: this is the one part of the 最新 sort
 * that can be pinned by a test without a connected client, so the component
 * calls it instead of inlining the comparison.
 */

/** The one field the 最新 ordering reads (a `StoreItem` satisfies it). */
export interface PublishDated {
  published_at?: string | null;
}

/**
 * 最新: dated entries first (newest first), then the undated ones; within each
 * group the rendered name decides, so the order is deterministic.
 *
 * `YYYY-MM-DD` compares correctly as a plain string, so nothing is parsed here.
 * An entry the market gave no date is **not** treated as the oldest possible
 * date — it simply sorts after the dated ones, which keeps "no date declared"
 * visibly different from "declared a long time ago".
 */
export function sortNewestFirst<T extends PublishDated>(
  items: readonly T[],
  nameOf: (item: T) => string,
  lang: string,
): T[] {
  return [...items].sort((a, b) => {
    const left = a.published_at ?? "";
    const right = b.published_at ?? "";
    if (left !== right) {
      if (!left) return 1;
      if (!right) return -1;
      return right.localeCompare(left);
    }
    return nameOf(a).localeCompare(nameOf(b), lang);
  });
}

/**
 * Whether this listing carries any date at all.
 *
 * The 最新 control is offered only then: a sort option over a field no entry
 * has would be a control that does nothing, and today's real markets declare no
 * `publishedAt` at all.
 */
export function hasAnyPublishedAt(items: readonly PublishDated[]): boolean {
  return items.some((item) => Boolean(item.published_at));
}
