import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('sider history tabs mutual exclusion and empty states', () => {
  test('implements tab domain active check and mutual exclusion with dock routes', () => {
    const siderSource = readSource(new URL('./index.tsx', import.meta.url));

    expect(siderSource.includes('isTabDomainActive')).toBe(true);
    expect(siderSource.includes('isDockRoute')).toBe(true);
    expect(siderSource.includes("pathname.startsWith('/knowledge')")).toBe(true);
    expect(siderSource.includes("pathname.startsWith('/learn')")).toBe(true);
    expect(siderSource.includes("pathname === '/scheduled'")).toBe(true);
    expect(siderSource.includes("pathname.startsWith('/meeting')")).toBe(true);
    expect(siderSource.includes("pathname.startsWith('/eval')")).toBe(true);

    // Tab buttons use isTabDomainActive to determine primary active vs subdued indicator
    expect(siderSource.includes("isTabDomainActive('workspaces')")).toBe(true);
    expect(siderSource.includes("isTabDomainActive('video')")).toBe(true);
    expect(siderSource.includes("isTabDomainActive('companions')")).toBe(true);
    expect(siderSource.includes('bg-fill-2 text-t-secondary font-medium')).toBe(true);
  });

  test('synchronizes routes bidirectionally without trampling companion conversations', () => {
    const siderSource = readSource(new URL('./index.tsx', import.meta.url));

    expect(siderSource.includes("handleSelectHistoryTab('workspaces')")).toBe(true);
    expect(siderSource.includes("handleSelectHistoryTab('video')")).toBe(true);
    expect(siderSource.includes("handleSelectHistoryTab('companions')")).toBe(true);

    // Clicking workspaces tab while in session does not discard current conversation
    expect(siderSource.includes('if (!isSessionRoute)')).toBe(true);
    expect(siderSource.includes('if (!isVideoRoute)')).toBe(true);
  });

  test('adds narrow rail text truncation, titles, and motion to history tabs', () => {
    const siderSource = readSource(new URL('./index.tsx', import.meta.url));

    expect(siderSource.includes('truncate')).toBe(true);
    expect(siderSource.includes('overflow-hidden text-ellipsis')).toBe(true);
    expect(siderSource.includes('transition-all duration-180')).toBe(true);
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

  test('renders companion empty state when hideHeader is true', () => {
    const companionGroupSource = readSource(
      new URL('../../../pages/conversation/SessionList/CompanionSessionGroup.tsx', import.meta.url)
    );

    expect(companionGroupSource.includes('if (companions.length === 0)')).toBe(true);
    expect(companionGroupSource.includes('if (hideHeader)')).toBe(true);
    expect(companionGroupSource.includes("t('nomi.companions.emptyTitle'")).toBe(true);
    expect(companionGroupSource.includes("t('nomi.companions.emptyHint'")).toBe(true);
    expect(companionGroupSource.includes("t('nomi.companions.createTitle'")).toBe(true);
  });
});
