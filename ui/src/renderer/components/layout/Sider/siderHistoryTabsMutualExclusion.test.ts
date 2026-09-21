import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('sider history tabs mutual exclusion and empty states', () => {
  test('history tab pills follow the selected list tab without a two-phase route wait', () => {
    const siderSource = readSource(new URL('./index.tsx', import.meta.url));

    expect(siderSource.includes("pathname.startsWith('/knowledge')")).toBe(true);
    expect(siderSource.includes("pathname.startsWith('/learn')")).toBe(true);
    expect(siderSource.includes("pathname === '/scheduled'")).toBe(true);
    expect(siderSource.includes("pathname.startsWith('/meeting')")).toBe(true);
    expect(siderSource.includes("pathname.startsWith('/eval')")).toBe(true);

    expect(siderSource.includes('isTabDomainActive')).toBe(false);
    expect(siderSource.includes('bg-fill-2 text-t-secondary font-medium hover:text-t-primary')).toBe(false);
    expect(siderSource.includes('bg-fill-3 text-t-primary shadow-sm')).toBe(true);
    expect(siderSource.includes('font-semibold')).toBe(false);
  });

  test('synchronizes routes one-way without trampling companion conversations or navigating on tab click', () => {
    const siderSource = readSource(new URL('./index.tsx', import.meta.url));

    expect(siderSource.includes('historyTabAfterPathChange')).toBe(true);
    expect(siderSource.includes("handleSelectHistoryTab('workspaces')")).toBe(true);

    // Search entry selection switches drawer tab to workspaces
    expect(siderSource.includes("handleSelectHistoryTab('workspaces');\n    if (onSessionClick)")).toBe(true);

    // History tabs only switch the list; they must not navigate the main outlet
    expect(siderSource.includes('if (!isSessionRoute)')).toBe(false);
    expect(siderSource.includes('if (!isVideoRoute)')).toBe(false);
    expect(siderSource.includes('handleNewChat();')).toBe(false);
    expect(siderSource.includes('handleVideoGenerationHome();')).toBe(false);
  });

  test('adds narrow rail text truncation, titles, and color-only motion to history tabs', () => {
    const siderSource = readSource(new URL('./index.tsx', import.meta.url));

    expect(siderSource.includes('truncate')).toBe(true);
    expect(siderSource.includes('overflow-hidden text-ellipsis')).toBe(true);
    expect(siderSource.includes('transition-colors duration-180')).toBe(true);
    expect(siderSource.includes('transition-all duration-180')).toBe(false);
    expect(siderSource.includes("title={t('sessionList.projectsTab'")).toBe(true);
    expect(siderSource.includes("title={t('videoGeneration.nav.shortTitle'")).toBe(true);
    expect(siderSource.includes("title={t('nomi.shortTitle'")).toBe(true);
  });

  test('renders video list empty state with dedicated string key', () => {
    const videoGroupSource = readSource(
      new URL('./SiderNav/SiderVideoGenerationGroup.tsx', import.meta.url)
    );

    expect(videoGroupSource.includes("t('videoGeneration.nav.recentEmpty'")).toBe(true);
    expect(videoGroupSource.includes("t('videoGeneration.list.empty'")).toBe(false);
  });

  test('keeps history tab panels mounted so switching does not refetch an empty flash', () => {
    const siderSource = readSource(new URL('./index.tsx', import.meta.url));

    expect(siderSource.includes("activeHistoryTab === 'workspaces' &&")).toBe(false);
    expect(siderSource.includes("activeHistoryTab === 'companions' &&")).toBe(false);
    expect(siderSource.includes("activeHistoryTab === 'video' &&")).toBe(false);
    expect(siderSource.includes("hidden={activeHistoryTab !== 'workspaces'}")).toBe(true);
    expect(siderSource.includes("hidden={activeHistoryTab !== 'companions'}")).toBe(true);
    expect(siderSource.includes("hidden={activeHistoryTab !== 'video'}")).toBe(true);
  });

  test('renders companion empty state when hideHeader is true', () => {
    const companionGroupSource = readSource(
      new URL('../../../pages/conversation/SessionList/CompanionSessionGroup.tsx', import.meta.url)
    );

    expect(companionGroupSource.includes('if (companions.length === 0)')).toBe(true);
    expect(companionGroupSource.includes('if (hideHeader)')).toBe(true);
    expect(companionGroupSource.includes("t('nomi.companions.emptyTitle'")).toBe(true);
    expect(companionGroupSource.includes("t('nomi.companions.emptyHint'")).toBe(true);
    expect(companionGroupSource.includes("t('nomi.companions.createTitle'")).toBe(true);
    // Roster fetch starts empty; showing the create CTA while loading is the tab flash.
    expect(companionGroupSource.includes('const { companions, loading } = useCompanions()')).toBe(true);
    expect(companionGroupSource.includes('if (loading)')).toBe(true);
    expect(companionGroupSource.includes('cursor-pointer transition-all box-border')).toBe(false);
  });
});
