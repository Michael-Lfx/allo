const DYNAMIC_IMPORT_ERROR_PATTERN =
  /Failed to fetch dynamically imported module|Importing a module script failed|dynamically imported module|ChunkLoadError/i;

export const DYNAMIC_IMPORT_RETRY_STORAGE_KEY = 'flowy.dynamic-import-retry';
export const DYNAMIC_IMPORT_RETRY_WINDOW_MS = 10_000;

type RetryRecord = {
  href: string;
  at: number;
};

/**
 * React.lazy caches a rejected loader promise. A state-only boundary reset
 * therefore renders the same error again; classify those failures so the
 * user can reload the module graph instead of being offered a misleading
 * in-place retry.
 */
export function isDynamicImportFailure(error: unknown): boolean {
  if (!error || typeof error !== 'object') return false;
  const message = 'message' in error && typeof error.message === 'string' ? error.message : '';
  return DYNAMIC_IMPORT_ERROR_PATTERN.test(message);
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
  try {
    const raw = storage?.getItem(DYNAMIC_IMPORT_RETRY_STORAGE_KEY);
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
    storage?.setItem(DYNAMIC_IMPORT_RETRY_STORAGE_KEY, JSON.stringify({ href, at: now }));
  } catch {
    // A blocked or unavailable storage area must not prevent a recovery reload.
  }
  return true;
}
