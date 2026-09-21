export type SiderHistoryTab = 'workspaces' | 'companions' | 'video';

/**
 * One-way route → tab mapping. Pathname changes can follow the user into a
 * domain; tab-only clicks must not be overwritten, so callers invoke this only
 * when `pathname` changes and must keep `companions` on session/guid routes.
 */
export function historyTabAfterPathChange(
  pathname: string,
  current: SiderHistoryTab,
): SiderHistoryTab {
  if (pathname.startsWith('/video-generation')) return 'video';
  if (pathname.startsWith('/nomi')) return 'companions';
  if (pathname.startsWith('/conversation/')) {
    return current === 'companions' ? 'companions' : 'workspaces';
  }
  if (
    pathname === '/guid' ||
    pathname === '/terminal-new' ||
    pathname.startsWith('/terminal/')
  ) {
    return current === 'companions' ? 'companions' : 'workspaces';
  }
  return current;
}
