import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const transitionSource = readFileSync(new URL('./SettingsNavigationTransition.tsx', import.meta.url), 'utf8');
const layoutSource = readFileSync(new URL('./Layout.tsx', import.meta.url), 'utf8');
const routerSource = readFileSync(new URL('./Router.tsx', import.meta.url), 'utf8');
const wrapperSource = readFileSync(
  new URL('../../pages/settings/components/SettingsPageWrapper.tsx', import.meta.url),
  'utf8'
);
const capabilityHubShellSource = readFileSync(
  new URL('../../pages/settings/capabilityHub/CapabilityHubShell.tsx', import.meta.url),
  'utf8'
);

describe('settings navigation loading contract', () => {
  test('paints the right-side loading layer before deferred navigation and recovers stale requests', () => {
    expect(transitionSource).toContain('SettingsNavigationTransitionProvider');
    expect(transitionSource).toContain('window.requestAnimationFrame');
    expect(transitionSource).toContain('SETTINGS_TRANSITION_BACKSTOP_MS');
    expect(transitionSource).toContain('pendingRef.current?.token !== next.token');
    expect(transitionSource).toContain("data-testid='settings-navigation-loading'");
    expect(transitionSource).toContain("variant='drive'");
    expect(layoutSource).toContain('<SettingsNavigationLoadingOverlay />');
    expect(layoutSource).toContain("className={'relative bg-base layout-content flex flex-col min-h-0'}");
  });

  test('uses the shared settings fallback for route chunks and marks committed routes ready', () => {
    expect(routerSource).toContain("const SETTINGS_CAPABILITY_PATHS = ['/presets', '/skills', '/mcp', '/plugins']");
    expect(routerSource).toContain("pathname.startsWith('/settings/')");
    expect(routerSource).toContain('<SettingsContentLoading />');
    expect(routerSource).toContain('markSettingsNavigationReady();');
    expect(routerSource).toContain('useLayoutEffect');
  });

  test('keeps mobile navigation available while a page-level data request is loading', () => {
    expect(wrapperSource).toContain('loading?: boolean');
    expect(wrapperSource).toContain('navigateWithSettingsTransition');
    expect(wrapperSource).toContain('pendingTarget');
    expect(wrapperSource).toContain('aria-busy={loading || undefined}');
    expect(wrapperSource).toContain('{loading ? <SettingsContentLoading /> : children}');
    expect(capabilityHubShellSource).toContain('parseCapabilityHubFromPathname');
    expect(capabilityHubShellSource).toContain('pendingTarget');
    expect(capabilityHubShellSource).toContain('hub={activeHub}');
  });
});
