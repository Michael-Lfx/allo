export type SiderHistoryTab = 'workspaces' | 'companions' | 'video';

/** Session surfaces (chat, guid, terminals) share the work/companion filters. */
export function isSessionRoutePath(pathname: string): boolean {
  return (
    pathname === '/guid' ||
    pathname.startsWith('/conversation/') ||
    pathname === '/terminal-new' ||
    pathname.startsWith('/terminal/')
  );
}

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
  if (isSessionRoutePath(pathname)) {
    return current === 'companions' ? 'companions' : 'workspaces';
  }
  return current;
}

/**
 * Which history tab the pill highlights for the current route. Session routes
 * follow the active list filter, so a companion conversation keeps 桌宠 lit
 * instead of stealing the 项目 highlight (the project list excludes companion
 * rows on purpose).
 */
export function activeHistoryTabForRoute(
  pathname: string,
  activeHistoryTab: SiderHistoryTab,
): SiderHistoryTab | null {
  if (isSessionRoutePath(pathname)) {
    return activeHistoryTab === 'companions' ? 'companions' : 'workspaces';
  }
  if (pathname.startsWith('/video-generation')) return 'video';
  if (pathname.startsWith('/nomi')) return 'companions';
  return null;
}
