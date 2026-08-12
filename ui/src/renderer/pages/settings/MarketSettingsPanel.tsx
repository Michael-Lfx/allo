/**
 * MarketSettingsPanel — shared ranking-market surface for the skill, MCP,
 * plugin, and preset-package markets. Renders the source switcher, sync /
 * search controls, the card grid, local item details, and (optionally) the
 * shared audience / scenario tag filter bar. Consumers own the primary action
 * meaning and security boundary via `primaryAction`.
 */
import type { ISkillMarketItem, SkillMarketSource } from '@/common/adapter/ipcBridge';
import { resolveLocaleKey } from '@/common/utils';
import { useLayoutContext } from '@/renderer/hooks/context/LayoutContext';
import { usePresetTags } from '@/renderer/hooks/preset';
import { openExternalUrl } from '@/renderer/utils/platform';
import { copyText } from '@/renderer/utils/ui/clipboard';
import { useArcoMessage } from '@/renderer/utils/ui/useArcoMessage';
import SkillMarketCard from './skill/SkillMarketCard';
import MarketDetailDrawer from './skill/MarketDetailDrawer';
import MarketToolbar from './skill/MarketToolbar';
import MarketCardGrid from './skill/MarketCardGrid';
import { createMarketItemViewModel, type MarketItemViewModel } from './skill/marketViewModel';
import type { MarketPrimaryActionConfig } from './skill/marketContracts';
import { useMarketActionState } from './skill/useMarketActionState';
import {
  marketSourceLabel,
  marketSourceUrl,
} from './skill/skillMarket';
import { useMarketCatalog } from './skill/useMarketCatalog';
import { Button } from '@arco-design/web-react';
import { LinkOne } from '@icon-park/react';
import React, { useCallback, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

/** Per-market wording overrides; every field falls back to the generic `settings.market.*` copy. */
type MarketPanelTextOverrides = {
  syncSuccess?: string;
  syncKeptCache?: string;
  syncEmpty?: string;
  syncError?: string;
  openFailed?: string;
  openInBrowser?: string;
  /** Shown when a search query matches nothing in the active source. */
  noSearchMatch?: (query: string, sourceLabel: string) => string;
  /** Shown when items exist but the active filters exclude them all. */
  noFilterMatch?: string;
  lastUpdated?: (time: string) => string;
};

type MarketSettingsPanelProps = {
  title: string;
  description: string;
  sources: SkillMarketSource[];
  cacheKey: string;
  autoSyncKey: string;
  defaultSource: SkillMarketSource;
  searchPlaceholder: string;
  emptyText: string;
  primaryAction: MarketPrimaryActionConfig;
  /** Render the shared audience/scenario tag filter bar (used by the skill market). */
  enableTagFilter?: boolean;
  /**
   * Stable id fragment for e2e hooks, e.g. `skill-market` keeps the legacy
   * `btn-sync-skill-market` ids. Omit to render without test ids.
   */
  testIdPrefix?: string;
  text?: MarketPanelTextOverrides;
};

/**
 * The market only owns transient interaction state.  Each consumer retains
 * its own security boundary and business flow through this small contract.
 */
export type { MarketPrimaryActionConfig } from './skill/marketContracts';

const MarketSettingsPanel: React.FC<MarketSettingsPanelProps> = ({
  title,
  description,
  sources,
  cacheKey,
  autoSyncKey,
  defaultSource,
  searchPlaceholder,
  emptyText,
  primaryAction,
  enableTagFilter = false,
  testIdPrefix,
  text,
}) => {
  const { t, i18n } = useTranslation();
  const localeKey = resolveLocaleKey(i18n.language);
  const layout = useLayoutContext();
  const isMobile = layout?.isMobile ?? false;
  const tags = usePresetTags();
  const [message, messageContext] = useArcoMessage({ maxCount: 10 });
  const [detailItem, setDetailItem] = useState<MarketItemViewModel | null>(null);
  const detailTriggerRef = useRef<HTMLElement | null>(null);

  const notify = useMemo(
    () => ({
      success: (value: string) => message.success(value),
      warning: (value: string) => message.warning(value),
      error: (value: string) => message.error(value),
    }),
    [message],
  );
  const catalog = useMarketCatalog({
    sources,
    cacheKey,
    autoSyncKey,
    defaultSource,
    enableTagFilter,
    tags,
    t,
    notify,
    text,
  });
  const {
    activeSource,
    setActiveSource,
    items,
    fetchedAt,
    errors,
    loading,
    searchQuery,
    setSearchQuery,
    searchExpanded,
    setSearchExpanded,
    tagFilter,
    setTagFilter,
    filteredItems,
    status,
    sourceCounts,
    syncMarket,
  } = catalog;

  const testId = useCallback(
    (id: string): string | undefined => (testIdPrefix ? id.replace('{market}', testIdPrefix) : undefined),
    [testIdPrefix]
  );

  const handleOpenMarket = useCallback(async () => {
    try {
      await openExternalUrl(marketSourceUrl(activeSource));
    } catch (error) {
      console.error('Failed to open market:', error);
      message.error(text?.openFailed ?? t('settings.market.openFailed', { defaultValue: '无法打开市场' }));
    }
  }, [activeSource, message, t, text]);

  const handleCopyInstallCommand = useCallback(
    async (item: ISkillMarketItem | MarketItemViewModel) => {
      try {
        await copyText('raw' in item ? item.installCommand : item.install_command);
        message.success(t('common.copySuccess', { defaultValue: '已复制' }));
      } catch (error) {
        console.error('Failed to copy market install command:', error);
        message.error(t('common.copyFailed', { defaultValue: '复制失败' }));
      }
    },
    [message, t]
  );

  const handleOpenItemSource = useCallback(
    async (item: ISkillMarketItem | MarketItemViewModel) => {
      try {
        await openExternalUrl('raw' in item ? item.sourceUrl : item.url);
      } catch (error) {
        console.error('Failed to open market item source:', error);
        message.error(text?.openFailed ?? t('settings.market.openFailed', { defaultValue: '无法打开市场' }));
      }
    },
    [message, t, text]
  );

  const actionState = useMarketActionState(primaryAction);

  const viewModels = useMemo(
    () =>
      filteredItems.map((item) =>
        createMarketItemViewModel(item, {
          localeKey,
          tagByKey: tags.tagByKey,
          t,
        })
      ),
    [filteredItems, localeKey, t, tags.tagByKey]
  );

  // The single activeActionItemId lives in useMarketActionState so cards and
  // the detail drawer cannot diverge during an install/import flow.
  const activeSearch = searchQuery.trim().length > 0;
  const resolvedEmptyText = loading
    ? t('common.loading', { defaultValue: '加载中...' })
    : activeSearch
      ? (text?.noSearchMatch?.(searchQuery.trim(), marketSourceLabel(activeSource)) ??
        t('settings.market.noSearchMatch', {
          query: searchQuery.trim(),
          source: marketSourceLabel(activeSource),
          defaultValue: `${marketSourceLabel(activeSource)} 中未找到与“${searchQuery.trim()}”相关的条目。`,
        }))
      : items.length === 0
        ? emptyText
        : (text?.noFilterMatch ?? emptyText);

  return (
    <div data-market-status={status}>
      {messageContext}
      <MarketToolbar
        title={title}
        description={description}
        sources={sources}
        activeSource={activeSource}
        sourceCounts={sourceCounts}
        onSourceChange={setActiveSource}
        loading={loading}
        onRefresh={() => void syncMarket()}
        searchPlaceholder={searchPlaceholder}
        searchQuery={searchQuery}
        searchExpanded={searchExpanded}
        onSearchQueryChange={setSearchQuery}
        onSearchExpandedChange={(expanded) => {
          setSearchExpanded(expanded);
          if (!expanded) setSearchQuery('');
        }}
        isMobile={isMobile}
        testId={testId}
        enableTagFilter={enableTagFilter}
        audienceTags={tags.audienceTags}
        scenarioTags={tags.scenarioTags}
        tagFilter={tagFilter}
        onTagFilterChange={setTagFilter}
        localeKey={localeKey}
      />

      {errors.length > 0 && (
        <div className='mb-14px rounded-12px border border-solid border-[rgba(var(--orange-6),0.24)] bg-[rgba(var(--orange-6),0.08)] px-14px py-10px text-12px leading-18px text-[rgb(var(--orange-7))]'>
          {errors.join(' / ')}
        </div>
      )}

      {filteredItems.length > 0 ? (
        <MarketCardGrid busy={loading}>
          {viewModels.map((item) => (
            <SkillMarketCard
              key={item.id}
              item={item}
              actionLabel={primaryAction.label}
              pendingLabel={primaryAction.pendingLabel}
              completedLabel={primaryAction.completedLabel}
              actionState={actionState.getState(item)}
              busy={actionState.isBusy(item.id)}
              disabled={actionState.isDisabled(item)}
              onAdd={(marketItem) => void actionState.runPrimaryAction(marketItem)}
              onOpenSource={(marketItem) => void handleOpenItemSource(marketItem)}
              onCopyInstallCommand={(marketItem) => void handleCopyInstallCommand(marketItem)}
              onViewDetails={(marketItem, trigger) => {
                detailTriggerRef.current = trigger ?? null;
                setDetailItem(marketItem);
              }}
            />
          ))}
        </MarketCardGrid>
      ) : loading && items.length === 0 ? (
        <div
          aria-busy='true'
          className='grid items-start gap-12px'
          style={{ gridTemplateColumns: 'repeat(auto-fit, minmax(min(270px, 100%), 1fr))' }}
        >
          {Array.from({ length: 6 }, (_, index) => (
            <div
              key={index}
              className='h-184px rounded-16px border border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] p-14px'
            >
              <div className='h-34px w-full animate-pulse rounded-8px bg-[var(--color-fill-2)]' />
              <div className='mt-16px h-14px w-5/6 animate-pulse rounded bg-[var(--color-fill-2)]' />
              <div className='mt-8px h-14px w-3/5 animate-pulse rounded bg-[var(--color-fill-2)]' />
            </div>
          ))}
        </div>
      ) : (
        <div className='text-center text-t-secondary py-40px'>{resolvedEmptyText}</div>
      )}

      {(fetchedAt || items.length > 0) && (
        <div className='mt-16px flex items-center justify-between gap-12px text-12px text-t-tertiary'>
          <span>
            {fetchedAt
              ? (text?.lastUpdated?.(new Date(fetchedAt).toLocaleString()) ??
                t('settings.market.lastUpdated', {
                  time: new Date(fetchedAt).toLocaleString(),
                  defaultValue: '上次更新：{{time}}',
                }))
              : ''}
          </span>
          <Button
            type='text'
            size='mini'
            data-testid={testId('btn-open-{market}-browser')}
            className='!rounded-10px !px-10px !h-28px !text-12px !text-t-secondary hover:!bg-fill-1 hover:!text-t-primary'
            icon={<LinkOne size={14} fill='currentColor' />}
            onClick={() => void handleOpenMarket()}
          >
            {text?.openInBrowser ?? t('settings.market.openInBrowser', { defaultValue: '打开市场' })}
          </Button>
        </div>
      )}

      <MarketDetailDrawer
        item={detailItem}
        visible={detailItem !== null}
        action={primaryAction}
        actionState={detailItem ? actionState.getState(detailItem) : 'ready'}
        busy={detailItem ? actionState.isBusy(detailItem.id) : false}
        disabled={detailItem ? actionState.isDisabled(detailItem) : false}
        onPrimaryAction={(item) => void actionState.runPrimaryAction(item)}
        onCopyInstallCommand={(item) => void handleCopyInstallCommand(item)}
        onOpenSource={(item) => void handleOpenItemSource(item)}
        onClose={() => setDetailItem(null)}
        restoreFocusRef={detailTriggerRef}
      />
    </div>
  );
};

export default MarketSettingsPanel;
