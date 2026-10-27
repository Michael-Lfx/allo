import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('sider history tabs mutual exclusion and empty states', () => {
  test('history tab pills follow activeBar2Module and mutually exclude Dock rail routes', () => {
    const siderSource = readSource(new URL('./index.tsx', import.meta.url));

    expect(siderSource.includes("pathname.startsWith('/knowledge')")).toBe(true);
    expect(siderSource.includes("pathname.startsWith('/learn')")).toBe(true);
    expect(siderSource.includes("pathname === '/scheduled'")).toBe(true);
    expect(siderSource.includes("pathname.startsWith('/eval')")).toBe(true);

    expect(siderSource.includes('isTabDomainActive')).toBe(false);
    expect(siderSource.includes('bg-fill-2 text-t-secondary font-medium hover:text-t-primary')).toBe(false);
    expect(siderSource.includes('bg-fill-3 text-t-primary shadow-sm')).toBe(false);
    expect(siderSource.includes('font-semibold')).toBe(false);

    // Active and optimistic Bar 2 module drives the full-width video / companion rows
    expect(siderSource.includes('activeBar2Module')).toBe(true);
    expect(siderSource.includes('effectiveBar2Module')).toBe(true);
    expect(siderSource.includes('optimisticBar2Module')).toBe(true);
    expect(siderSource.includes("aria-selected={effectiveBar2Module === 'workspaces'}")).toBe(false);
    expect(siderSource.includes("active={effectiveBar2Module === 'video'}")).toBe(true);
    expect(siderSource.includes("active={effectiveBar2Module === 'companions'}")).toBe(true);
    expect(siderSource.includes("testId='sider-video-module-row'")).toBe(true);
    expect(siderSource.includes("testId='sider-companion-module-row'")).toBe(true);

    // Video, skill store, then companion, as full-width rows. Projects stay in the list below.
    expect(siderSource.includes('<MessageOne')).toBe(false);
    expect(siderSource.includes('<VideoOne')).toBe(true);
    expect(siderSource.includes('<Ghost')).toBe(true);
    expect(siderSource.includes('🐱')).toBe(false);
    const videoRow = siderSource.indexOf("testId='sider-video-module-row'");
    const skillStoreRow = siderSource.indexOf("testId='sider-skill-store-row'");
    const companionRow = siderSource.indexOf("testId='sider-companion-module-row'");
    const knowledgeRow = siderSource.indexOf('<SiderKnowledgeEntry');
    const learningRow = siderSource.indexOf('<SiderLearningEntry');
    const scheduledRow = siderSource.indexOf('<SiderScheduledEntry');
    expect(videoRow).toBeGreaterThan(-1);
    expect(videoRow).toBeLessThan(skillStoreRow);
    expect(skillStoreRow).toBeLessThan(companionRow);
    expect(companionRow).toBeLessThan(knowledgeRow);
    expect(knowledgeRow).toBeLessThan(learningRow);
    expect(learningRow).toBeLessThan(scheduledRow);
  });

  test('warms companion and video home on hover and app idle', () => {
    const siderSource = readSource(new URL('./index.tsx', import.meta.url));

    expect(siderSource.includes("import { prefetchNomiPage } from '@renderer/pages/nomi/prefetch'")).toBe(true);
    expect(
      siderSource.includes("import { prefetchVideoGenerationHome } from '@renderer/pages/videoGeneration/prefetch'")
    ).toBe(true);
    expect(siderSource.includes('prefetchNomiPage()')).toBe(true);
    expect(siderSource.includes('prefetchVideoGenerationHome()')).toBe(true);
    expect(siderSource.includes('onPointerEnter={() => prefetchNomiPage()}')).toBe(true);
    expect(siderSource.includes('onPointerEnter={() => prefetchVideoGenerationHome()}')).toBe(true);
  });

  test('synchronizes routes one-way and navigates module on tab click when not already on route', () => {
    const siderSource = readSource(new URL('./index.tsx', import.meta.url));

    expect(siderSource.includes('historyTabAfterPathChange')).toBe(true);
    expect(siderSource.includes("handleSelectHistoryTab('workspaces')")).toBe(true);

    // Search entry selection switches drawer tab to workspaces
    expect(siderSource.includes("handleSelectHistoryTab('workspaces');\n    if (onSessionClick)")).toBe(true);

    // Collapsed rail restores recent active conversation while top + starts a new chat
    expect(siderSource.includes('getRecentConversationPath')).toBe(true);
    expect(siderSource.includes('lastActiveConversationPathRef')).toBe(true);
    expect(siderSource.includes('handleConversationClick')).toBe(true);

    // Expanded workspaces tab navigates to recent conversation; video routes home
    expect(siderSource.includes("if (tab === 'workspaces')")).toBe(true);
    expect(siderSource.includes('if (!isVideoRoute)')).toBe(true);
    expect(siderSource.includes('handleVideoGenerationHome();')).toBe(true);
  });

  test('adds narrow rail text truncation, titles, and color-only motion to history tabs', () => {
    const siderSource = readSource(new URL('./index.tsx', import.meta.url));

    expect(siderSource.includes('truncate')).toBe(false);
    expect(siderSource.includes('overflow-hidden text-ellipsis')).toBe(false);
    expect(siderSource.includes('transition-colors duration-180')).toBe(false);
    expect(siderSource.includes('transition-all duration-180')).toBe(false);
    expect(siderSource.includes("title={t('sessionList.projectsTab'")).toBe(false);
    expect(siderSource.includes("label={t('videoGeneration.nav.shortTitle'")).toBe(true);
    expect(siderSource.includes("label={t('nomi.shortTitle'")).toBe(true);
    const moduleRowSource = readSource(new URL('./SiderNav/SiderModuleRow.tsx', import.meta.url));
    expect(moduleRowSource.includes('h-34px w-full flex items-center justify-start')).toBe(true);
    expect(moduleRowSource.includes('truncate')).toBe(true);
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

  test('unifies empty state rendering across workspaces, video, and companion tabs', () => {
    const placeholderSource = readSource(new URL('./SiderEmptyPlaceholder.tsx', import.meta.url));
    const videoGroupSource = readSource(new URL('./SiderNav/SiderVideoGenerationGroup.tsx', import.meta.url));
    const companionGroupSource = readSource(
      new URL('../../../pages/conversation/SessionList/CompanionSessionGroup.tsx', import.meta.url)
    );
    const workpathDrawerSource = readSource(
      new URL('../../../pages/conversation/SessionList/WorkpathDrawer.tsx', import.meta.url)
    );

    // All three tabs render the unified placeholder
    expect(videoGroupSource.includes('<SiderEmptyPlaceholder')).toBe(true);
    expect(companionGroupSource.includes('<SiderEmptyPlaceholder')).toBe(true);
    expect(workpathDrawerSource.includes('<SiderEmptyPlaceholder')).toBe(true);

    // SiderEmptyPlaceholder enforces the design spec: 28px height, 8px radius, theme tokens
    expect(placeholderSource.includes('h-28px px-12px rd-8px')).toBe(true);
    expect(placeholderSource.includes('bg-fill-2 hover:bg-fill-3 active:bg-fill-4')).toBe(true);
    expect(placeholderSource.includes('border-[var(--color-border-2)]')).toBe(true);

    // Companion tab preserves the distinctive cat emoji
    expect(companionGroupSource.includes('🐱')).toBe(true);

    // Workpath drawer renders localized empty text and initiates conversation
    expect(workpathDrawerSource.includes("t('sessionList.drawerEmpty'")).toBe(true);
    expect(workpathDrawerSource.includes("onCreateInteractive(node)")).toBe(true);
  });
});
