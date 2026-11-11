const DYNAMIC_IMPORT_ERROR_PATTERN =
  /Failed to fetch dynamically imported module|Importing a module script failed|dynamically imported module|ChunkLoadError|does not provide an export named/i;

const DOM_COMMIT_OPERATIONS_PATTERN = /(removeChild|insertBefore|replaceChild|appendChild)/i;
const DOM_COMMIT_DETAIL_PATTERN = /not a child|not a descendant|contains the parent/i;
const DOM_COMMIT_EXCEPTION_NAMES = new Set(['NotFoundError', 'HierarchyRequestError']);

export const DYNAMIC_IMPORT_RETRY_STORAGE_KEY = 'flowy.dynamic-import-retry';
export const DYNAMIC_IMPORT_RETRY_WINDOW_MS = 10_000;

type RetryRecord = {
  href: string;
  at: number;
};

/**
 * React.lazy caches a rejected loader promise. A state-only boundary reset
 * therefore renders the same error again. That includes stale HMR graphs
 * where a module no longer provides an export the loader already imported.
 * Classify those failures so the user can reload the module graph instead of
 * being offered a misleading in-place retry.
 */
export function isDynamicImportFailure(error: unknown): boolean {
  if (!error || typeof error !== 'object') return false;
  const message = 'message' in error && typeof error.message === 'string' ? error.message : '';
  return DYNAMIC_IMPORT_ERROR_PATTERN.test(message);
}

/**
 * DOM commit failures happen when a node React owns is detached by something
 * else before React's next mutation commit (browser automation writing a
 * contentEditable value through UIA, translate extensions, …). React's
 * `removeChild`/`insertBefore` then throws `NotFoundError` during the commit
 * phase. The failed subtree is unmounted by the boundary, so remounting it once
 * starts from a clean DOM and recovers without a full page reload.
 *
 * Chromium's DOMException message text is not part of a stable contract, so the
 * DOMException name is the primary signal; the message patterns stay as a
 * fallback for engines/wrappers that only forward the text.
 */
export function isDomCommitFailure(error: unknown): boolean {
  if (!error || typeof error !== 'object') return false;
  const message = 'message' in error && typeof error.message === 'string' ? error.message : '';
  const name = 'name' in error && typeof error.name === 'string' ? error.name : '';
  if (DOM_COMMIT_EXCEPTION_NAMES.has(name)) {
    return true;
  }
  return DOM_COMMIT_OPERATIONS_PATTERN.test(message) && DOM_COMMIT_DETAIL_PATTERN.test(message);
}

/**
 * Allow one reload for a given URL within a short window. If the module is
 * still unavailable after that reload, the boundary remains visible and the
 * next reload is explicitly user-triggered instead of entering a reload loop.
 */
export function claimDynamicImportReload(
  storage: Pick<Storage, 'getItem' | 'setItem'> | null | undefined,
  href: string,
  now = Date.now(),
): boolean {
  if (!storage) return false;

  try {
    const raw = storage.getItem(DYNAMIC_IMPORT_RETRY_STORAGE_KEY);
    if (raw) {
      const previous = JSON.parse(raw) as Partial<RetryRecord>;
      if (
        previous.href === href &&
        typeof previous.at === 'number' &&
        Math.abs(now - previous.at) < DYNAMIC_IMPORT_RETRY_WINDOW_MS
      ) {
        return false;
      }
    }
    storage.setItem(DYNAMIC_IMPORT_RETRY_STORAGE_KEY, JSON.stringify({ href, at: now }));
    return true;
  } catch {
    // Without a durable claim, offering another automatic reload can loop on a
    // persistently stale module graph. Let the boundary expose manual reload.
    return false;
  }
}
