export const LAST_NON_SETTINGS_PATH_KEY = 'nomi:last-non-settings-path';

export function readLastNonSettingsPath(): string {
  try {
    const stored = sessionStorage.getItem(LAST_NON_SETTINGS_PATH_KEY);
    if (stored && !stored.startsWith('/settings')) {
      return stored;
    }
  } catch {
    // ignore
  }
  return '/guid';
}

export function resolveSettingsTogglePath(
  pathname: string,
  lastNonSettingsPath?: string
): { enter: boolean; path: string } {
  if (pathname.startsWith('/settings')) {
    const fallback = lastNonSettingsPath && !lastNonSettingsPath.startsWith('/settings')
      ? lastNonSettingsPath
      : readLastNonSettingsPath();
    return { enter: false, path: fallback || '/guid' };
  }
  return { enter: true, path: '/settings/system' };
}
