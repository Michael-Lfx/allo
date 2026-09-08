import type { ISkillMarketItem } from '@/common/adapter/ipcBridge';
import { ipcBridge } from '@/common';
import { isBackendHttpError } from '@/common/adapter/httpBridge';
import { useLayoutContext } from '@/renderer/hooks/context/LayoutContext';
import { defaultSkillHubMarketSource, useSkillHubMarket } from '@/renderer/pages/settings/skill/useSkillHubMarket';
import { openExternalUrl } from '@/renderer/utils/platform';
import { useArcoMessage } from '@/renderer/utils/ui/useArcoMessage';
import { Button } from '@arco-design/web-react';
import { LinkOne, Refresh } from '@icon-park/react';
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import useSWR from 'swr';
import { useTranslation } from 'react-i18next';
import { AVAILABLE_SKILLS_SWR_KEY, fetchAvailableSkills } from './skill/availableSkills';
import MarketCardGrid from './skill/MarketCardGrid';
import MarketDetailDrawer from './skill/MarketDetailDrawer';
import SkillHubMarketToolbar from './skill/SkillHubMarketToolbar';
import SkillMarketListRow from './skill/SkillMarketListRow';
import SkillMarketCard from './skill/SkillMarketCard';
import type { MarketPrimaryActionConfig } from './skill/marketContracts';
import { isSkillMarketItemInstalled, marketSkillInstallErrorMessage } from './skill/skillMarket';
import { createSkillHubMarketItemViewModel, type MarketItemViewModel } from './skill/marketViewModel';
import { useMarketActionState } from './skill/useMarketActionState';
import { notifySkillCatalogChanged } from '@/renderer/hooks/skills/skillCatalogEvents';

type SkillHubMarketPanelProps = {
  active?: boolean;
  hideSearch?: boolean;
  searchQuery?: string;
  onSearchQueryChange?: (value: string) => void;
};

const formatMarketTime = (value: number | null | undefined, locale: string): string => {
  if (!value) return '';
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? '' : date.toLocaleString(locale);
};

const SkillHubMarketPanel: React.FC<SkillHubMarketPanelProps> = ({
  active = true,
  searchQuery: searchQueryProp,
}) => {
  const { t, i18n } = useTranslation();
  const layout = useLayoutContext();
  const isMobile = layout?.isMobile ?? false;
  const locale = i18n.language || 'zh-CN';
  const isChinese = locale.toLowerCase().startsWith('zh');
  const defaultSource = defaultSkillHubMarketSource(locale);
  const [message, messageContext] = useArcoMessage({ maxCount: 10 });
  const [detailItem, setDetailItem] = useState<MarketItemViewModel | null>(null);
  const [installedMarketIds, setInstalledMarketIds] = useState<Set<string>>(() => new Set());
  const detailTriggerRef = useRef<HTMLElement | null>(null);
  const { data: skills, error: installedError, isLoading: installedLoading, mutate } = useSWR(
    active ? AVAILABLE_SKILLS_SWR_KEY : null,
    fetchAvailableSkills,
  );
  const installedSkillNames = useMemo(
    () => new Set((skills ?? []).map((skill) => skill.name)),
    [skills],
  );
  const installedStateLoading = Boolean(active && installedLoading && !skills);
  const installedStateAvailable = Boolean(skills) && !installedError;
  const market = useSkillHubMarket({ enabled: active, defaultSource });
  const selectedSourceLabel = market.source === 'clawhub'
    ? 'ClawHub'
    : market.source === 'skillhub'
      ? 'SkillHub'
      : t('settings.skillsMarket.allSources', { defaultValue: '全部来源' });
  const [viewMode, setViewMode] = useState<'list' | 'grid'>('list');
  const { beginInstallation, markInstalled, clearInstallationStatus } = market;

  useEffect(() => {
    if (searchQueryProp === undefined || searchQueryProp === market.searchQuery) return;
    market.setSearchQuery(searchQueryProp);
  }, [market.searchQuery, market.setSearchQuery, searchQueryProp]);

  const isAdded = useCallback(
    (item: ISkillMarketItem) => installedMarketIds.has(item.id)
      || (skills ?? []).some((skill) => skill.market_id === item.id)
      || isSkillMarketItemInstalled(item, installedSkillNames),
    [installedMarketIds, installedSkillNames, skills],
  );

  const handleAdd = useCallback(
    async (item: ISkillMarketItem) => {
      beginInstallation(item.id);
      try {
        const result = await ipcBridge.fs.installSkillMarketSkill.invoke({
          source: 'skillhub',
          id: item.id,
          market_source: item.market_source,
        });
        setInstalledMarketIds((current) => {
          if (current.has(item.id)) return current;
          const next = new Set(current);
          next.add(item.id);
          return next;
        });
        markInstalled();
        try {
          await mutate();
        } catch (refreshError) {
          console.warn('SkillHub installed, but available-skill refresh failed:', refreshError);
        }
        notifySkillCatalogChanged();
        message.success(
          result.status === 'reused'
            ? t('settings.skillsMarket.installReused', { defaultValue: '技能已在技能库中' })
            : t('settings.skillsMarket.installSuccess', {
                defaultValue: '已安装到技能库，可在预设编辑器中选择',
              }),
        );
      } catch (error) {
        clearInstallationStatus();
        console.error('Failed to install SkillHub skill:', error);
        const mapped = marketSkillInstallErrorMessage(isBackendHttpError(error) ? error.code : '');
        message.error(t(mapped.key, { defaultValue: mapped.fallback }));
      }
    }, [beginInstallation, clearInstallationStatus, markInstalled, message, mutate, t],
  );

  const primaryAction = useMemo<MarketPrimaryActionConfig>(
    () => ({
      label: t('settings.market.install', { defaultValue: '安装' }),
      pendingLabel: t('settings.market.installing', { defaultValue: '安装中' }),
      completedLabel: t('settings.market.installed', { defaultValue: '已安装' }),
      resolveState: (item) =>
        installedStateLoading
          ? 'checking'
          : installedStateAvailable && isAdded(item)
            ? 'completed'
            : 'ready',
      run: handleAdd,
    }),
    [handleAdd, installedStateAvailable, installedStateLoading, isAdded, t],
  );
  const actionState = useMarketActionState(primaryAction);

  const categoryLabel = useCallback(
    (category: { name: string; name_en: string }) => (isChinese ? category.name : category.name_en),
    [isChinese],
  );
  const categoryLabelByKey = useMemo(
    () => new Map(market.categories.map((category) => [category.key, categoryLabel(category)])),
    [categoryLabel, market.categories],
  );
  const skillCategoryLabel = useCallback(
    (key: string) => categoryLabelByKey.get(key) ?? key,
    [categoryLabelByKey],
  );

  const viewModels = useMemo(
    () => market.items.map((item) => createSkillHubMarketItemViewModel(item, {
      localeKey: locale,
      t,
      categoryLabel: skillCategoryLabel,
    })),
    [locale, market.items, skillCategoryLabel, t],
  );

  const handleOpenSource = useCallback(
    async (item: MarketItemViewModel) => {
      try {
        await openExternalUrl(item.sourceUrl);
      } catch (error) {
        console.error('Failed to open SkillHub source:', error);
        message.error(t('settings.skillsMarket.openMarketFailed', { defaultValue: '无法打开 SkillHub' }));
      }
    },
    [message, t],
  );

  const emptyText = market.status === 'error'
    ? t('settings.skillsMarket.sourceLoadError', {
        source: selectedSourceLabel,
        defaultValue: '{{source}} 内容暂时无法访问，请重试。',
      })
    : market.keyword
      ? t('settings.skillsMarket.noSearchMatch', {
          query: market.keyword,
          source: selectedSourceLabel,
          defaultValue: '{{source}} 中未找到与“{{query}}”相关的技能。',
        })
      : t('settings.skillsMarket.empty', { defaultValue: `暂未获取到 ${selectedSourceLabel} 技能，请点击刷新重试。` });
  const showMarketErrorBanner = (market.status === 'stale' || market.status === 'partial-error') && Boolean(market.error);

  const categoryOptions = useMemo(
    () => market.categories.map((category) => ({ value: category.key, label: categoryLabel(category) })),
    [categoryLabel, market.categories],
  );

  const marketBrowserUrl = useMemo(() => {
    const params = new URLSearchParams({ sortBy: market.sortBy, order: 'desc' });
    if (market.source === 'skillhub') params.set('source', 'community');
    if (market.source === 'clawhub') params.set('source', 'clawhub');
    if (market.category) params.set('category', market.category);
    if (market.requiresApiKey !== undefined) {
      params.set('labels', `requires_api_key:${market.requiresApiKey}`);
    }
    if (market.keyword) params.set('keyword', market.keyword);
    return `https://skillhub.cn/skills?${params.toString()}`;
  }, [market.category, market.keyword, market.requiresApiKey, market.sortBy, market.source]);

  return (
    <div className='w-full pb-16px' data-market-status={`${market.dataStatus}:${market.installationStatus}`}>
      {messageContext}
      <div className='mb-16px flex flex-col gap-12px pt-4px'>
        <div className={`flex gap-12px ${isMobile ? 'flex-col' : 'items-start justify-between'}`}>
          <div className='min-w-0'>
            <h2 className='m-0 text-18px font-semibold text-t-primary'>
              {t('settings.skillsMarket.title', { defaultValue: '技能市场' })}
            </h2>
            <p className='m-0 mt-4px max-w-[720px] text-13px leading-20px text-t-tertiary'>
              {t('settings.skillsMarket.description', {
                defaultValue: '仅展示 SkillHub 技能，可按分类、API Key 条件、热度和更新时间筛选。',
              })}
            </p>
          </div>
          <div className='flex shrink-0 items-center gap-8px'>
            <Button
              type='text'
              size='small'
              data-testid='btn-sync-skill-market'
              className='flex !h-34px !w-34px items-center justify-center !rounded-10px !p-0 !text-t-secondary hover:!bg-fill-1 hover:!text-t-primary'
              icon={<Refresh size={16} fill='currentColor' className={market.loading ? 'animate-spin' : ''} />}
              onClick={() => void market.refresh()}
              title={t('common.refresh', { defaultValue: '刷新' })}
            />
          </div>
        </div>
      </div>

      <SkillHubMarketToolbar
        source={market.source}
        onSourceChange={market.setSource}
        category={market.category}
        categoryOptions={categoryOptions}
        onCategoryChange={market.setCategory}
        requiresApiKey={market.requiresApiKey}
        onRequiresApiKeyChange={market.setRequiresApiKey}
        sortBy={market.sortBy}
        onSortChange={market.setSortBy}
        viewMode={viewMode}
        onViewModeChange={setViewMode}
        categoriesLoading={market.categoriesLoading}
        categoriesError={Boolean(market.categoriesError)}
        onRetryCategories={() => void market.retryCategories()}
        t={t}
      />

      <div className='mb-12px' />

      {market.categoriesError && (
        <div className='mb-12px text-12px text-t-tertiary'>
          {t('settings.skillsMarket.categoryLoadFailed', { defaultValue: '分类暂时不可用，列表仍可浏览。' })}
        </div>
      )}
      {showMarketErrorBanner && (
        <div className='mb-14px rounded-12px border border-solid border-[rgba(var(--orange-6),0.24)] bg-[rgba(var(--orange-6),0.08)] px-14px py-10px text-12px leading-18px text-warning-6'>
          {market.status === 'partial-error'
            ? t('settings.skillsMarket.partialError', { defaultValue: '部分结果加载失败，请重试加载更多。' })
            : t('settings.skillsMarket.stale', { defaultValue: '当前显示的是缓存数据，可能已过期。' })}
        </div>
      )}

      {viewModels.length > 0 ? (
        viewMode === 'list' ? (
          <div className='min-w-0' data-testid='skillhub-market-list'>
            {viewModels.map((item) => (
              <SkillMarketListRow
                key={item.id}
                item={item}
                actionLabel={primaryAction.label}
                pendingLabel={primaryAction.pendingLabel}
                completedLabel={primaryAction.completedLabel}
                actionState={actionState.getState(item)}
                busy={actionState.isBusy(item.id)}
                disabled={actionState.isDisabled(item)}
                onAdd={(marketItem) => void actionState.runPrimaryAction(marketItem)}
                onOpenSource={(marketItem) => void handleOpenSource(marketItem)}
                onViewDetails={(marketItem, trigger) => {
                  detailTriggerRef.current = trigger ?? null;
                  setDetailItem(marketItem);
                }}
              />
            ))}
          </div>
        ) : (
          <MarketCardGrid busy={market.loading}>
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
                showInstallCommand={false}
                onAdd={(marketItem) => void actionState.runPrimaryAction(marketItem)}
                onOpenSource={(marketItem) => void handleOpenSource(marketItem)}
                onCopyInstallCommand={() => undefined}
                onViewDetails={(marketItem, trigger) => {
                  detailTriggerRef.current = trigger ?? null;
                  setDetailItem(marketItem);
                }}
              />
            ))}
          </MarketCardGrid>
        )
      ) : market.loading ? (
        <div aria-busy='true' className='grid items-start gap-16px' style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(min(280px, 100%), 1fr))' }}>
          {Array.from({ length: 6 }, (_, index) => (
            <div key={index} className='h-220px rounded-12px border border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] p-18px'>
              <div className='h-34px w-full animate-pulse rounded-8px bg-[var(--color-fill-2)]' />
              <div className='mt-16px h-14px w-5/6 animate-pulse rounded bg-[var(--color-fill-2)]' />
              <div className='mt-8px h-14px w-3/5 animate-pulse rounded bg-[var(--color-fill-2)]' />
            </div>
          ))}
        </div>
      ) : (
        <div className='py-40px text-center text-t-secondary'>{emptyText}</div>
      )}

      {market.hasMore && (
        <div className='mt-16px flex justify-center'>
          <Button loading={market.loadingMore} onClick={() => void market.loadMore()}>
            {t('settings.skillsMarket.loadMore', { defaultValue: '加载更多' })}
          </Button>
        </div>
      )}

      {(market.fetchedAt || market.items.length > 0) && (
        <div className='mt-16px flex flex-wrap items-center justify-between gap-12px text-12px text-t-tertiary'>
          <span>
            {market.fetchedAt
              ? t('settings.skillsMarket.lastUpdated', {
                  time: formatMarketTime(market.fetchedAt, locale),
                  defaultValue: '上次更新：{{time}}',
                })
              : ''}
          </span>
          <Button
            type='text'
            size='mini'
            data-testid='btn-open-skill-market-browser'
            className='flowy-icon-text-btn !h-28px !rounded-10px !px-10px !text-12px !text-t-secondary hover:!bg-fill-1 hover:!text-t-primary'
            icon={<LinkOne size={14} fill='currentColor' />}
            onClick={() => void openExternalUrl(marketBrowserUrl)}
          >
            {t('settings.skillsMarket.openInBrowser', { defaultValue: '在浏览器中打开 SkillHub' })}
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
        onCopyInstallCommand={() => undefined}
        showInstallCommand={false}
        onOpenSource={(item) => void handleOpenSource(item)}
        onClose={() => setDetailItem(null)}
        restoreFocusRef={detailTriggerRef}
      />
    </div>
  );
};

export default SkillHubMarketPanel;
