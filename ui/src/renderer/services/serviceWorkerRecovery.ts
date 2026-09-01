export const SERVICE_WORKER_CLEANUP_RELOAD_STORAGE_KEY = 'flowy.service-worker-cleanup-reload';

type StorageLike = Pick<Storage, 'getItem' | 'setItem'>;

/**
 * Claim one cleanup reload for the current URL. Cleanup is intentionally
 * fail-closed when session storage is unavailable: this helper is used by an
 * automatic startup path, so reloading without a durable loop guard could
 * leave the app in an endless reload cycle.
 */
export function claimServiceWorkerCleanupReload(
  storage: StorageLike | null | undefined,
  href: string,
): boolean {
  if (!storage || !href) return false;
  try {
    if (storage.getItem(SERVICE_WORKER_CLEANUP_RELOAD_STORAGE_KEY) === href) return false;
    storage.setItem(SERVICE_WORKER_CLEANUP_RELOAD_STORAGE_KEY, href);
    return true;
  } catch {
    return false;
  }
}
