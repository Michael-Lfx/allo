import { ipcBridge } from '@/common';
import type { ISkillMarketItem, SkillMarketSource } from '@/common/adapter/ipcBridge';
import type { PresetTag } from '@/common/types/agent/presetTypes';
import type { TagFilterState } from '../PresetSettings/presetUtils';
import type { SkillTagFilterState } from './skillFilter';
import {
  cleanMarketText,
  filterSkillMarketItems,
  marketSourceLabel,
  normalizeSkillMarketErrors,
  normalizeSkillMarketItems,
  resolveMarketSyncItems,
  selectMarketSourceWithItems,
} from './skillMarket';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

type Translate = (key: string, options?: Record<string, unknown>) => string;

export type MarketCatalogText = {
  syncSuccess?: string;
  syncKeptCache?: string;
  syncEmpty?: string;
  syncError?: string;
};

export type MarketCatalogStatus =
  | 'loading'
  | 'cached-refresh'
  | 'ready'
  | 'empty'
  | 'no-match'
  | 'partial-error';

export type MarketCatalogNotifier = {
  success: (text: string) => void;
  warning: (text: string) => void;
  error: (text: string) => void;
};

type MarketCatalogTags = {
  audienceTags: PresetTag[];
  scenarioTags: PresetTag[];
};

type UseMarketCatalogOptions = {
  sources: SkillMarketSource[];
  cacheKey: string;
  autoSyncKey: string;
  defaultSource: SkillMarketSource;
  enableTagFilter: boolean;
  tags: MarketCatalogTags;
  t: Translate;
  notify: MarketCatalogNotifier;
  text?: MarketCatalogText;
};

/** Skip automatic ranking sync when a full local cache is newer than this. */
export const MARKET_CACHE_TTL_MS = 6 * 60 * 60 * 1000;

export const useMarketCatalog = ({
  sources,
  cacheKey,
  autoSyncKey,
  defaultSource,
  enableTagFilter,
  tags,
  t,
  notify,
  text,
}: UseMarketCatalogOptions) => {
  const autoSyncStartedRef = useRef(false);
  const itemsRef = useRef<ISkillMarketItem[]>([]);
  const [activeSource, setActiveSource] = useState<SkillMarketSource>(defaultSource);
  const [items, setItems] = useState<ISkillMarketItem[]>([]);
  const [fetchedAt, setFetchedAt] = useState<number | null>(null);
  const [errors, setErrors] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);
  const [searchQuery, setSearchQuery] = useState('');
  const [searchExpanded, setSearchExpanded] = useState(false);
  const [tagFilter, setTagFilter] = useState<TagFilterState>({ audience: [], scenario: [] });

  useEffect(() => {
    if (!enableTagFilter) return;
    const audienceIds = new Set(tags.audienceTags.map((tag) => tag.preset_tag_id));
    const scenarioIds = new Set(tags.scenarioTags.map((tag) => tag.preset_tag_id));
    setTagFilter((previous) => {
      const audience = previous.audience.filter((id) => audienceIds.has(id));
      const scenario = previous.scenario.filter((id) => scenarioIds.has(id));
      if (audience.length === previous.audience.length && scenario.length === previous.scenario.length) return previous;
      return { audience, scenario };
    });
  }, [enableTagFilter, tags.audienceTags, tags.scenarioTags]);

  const syncMarket = useCallback(
    async (options?: { showToast?: boolean }) => {
      const showToast = options?.showToast ?? true;
      setLoading(true);
      try {
        const result = await ipcBridge.fs.syncSkillMarketRankings.invoke({ sources });
        const normalized = normalizeSkillMarketItems(result.items).filter((item) => sources.includes(item.source));
        const normalizedErrors = normalizeSkillMarketErrors(result.errors);
        const nextItems = resolveMarketSyncItems(itemsRef.current, normalized);
        itemsRef.current = nextItems;
        setItems(nextItems);
        setActiveSource((source) => selectMarketSourceWithItems(source, sources, nextItems));
        setFetchedAt(result.fetched_at);
        setErrors(normalizedErrors);
        localStorage.setItem(cacheKey, JSON.stringify({ fetched_at: result.fetched_at, items: nextItems, errors: normalizedErrors }));

        if (showToast) {
          if (normalized.length > 0) {
            notify.success(text?.syncSuccess ?? t('settings.market.syncSuccess', { defaultValue: '市场已更新' }));
          } else if (nextItems.length > 0) {
            notify.warning(text?.syncKeptCache ?? t('settings.market.syncKeptCache', { defaultValue: '未获取到新数据，已保留本地缓存。' }));
          } else {
            notify.warning(text?.syncEmpty ?? t('settings.market.syncEmpty', { defaultValue: '未获取到市场数据。' }));
          }
        }
      } catch (error) {
        console.error('Failed to sync market:', error);
        const errorText = text?.syncError ?? t('settings.market.syncError', { defaultValue: '更新市场失败' });
        setErrors([errorText]);
        if (showToast) notify.error(errorText);
      } finally {
        setLoading(false);
      }
    },
    [cacheKey, notify, sources, t, text],
  );

  useEffect(() => {
    let cachedItems: ISkillMarketItem[] = [];
    let cachedFetchedAt: number | null = null;

    try {
      const raw = localStorage.getItem(cacheKey);
      if (raw) {
        const cache = JSON.parse(raw) as { fetched_at?: number; items?: unknown; errors?: unknown };
        cachedItems = normalizeSkillMarketItems(cache.items).filter((item) => sources.includes(item.source));
        cachedFetchedAt = typeof cache.fetched_at === 'number' ? cache.fetched_at : null;
        itemsRef.current = cachedItems;
        setItems(cachedItems);
        setActiveSource((source) => selectMarketSourceWithItems(source, sources, cachedItems));
        setFetchedAt(cachedFetchedAt);
        setErrors(normalizeSkillMarketErrors(cache.errors));
      }
    } catch {
      localStorage.removeItem(cacheKey);
    }

    if (autoSyncStartedRef.current) return;
    autoSyncStartedRef.current = true;

    const cacheIsFresh =
      cachedItems.length > 0 &&
      cachedFetchedAt !== null &&
      Date.now() - cachedFetchedAt < MARKET_CACHE_TTL_MS;

    try {
      if (sessionStorage.getItem(autoSyncKey) === '1') return;
      sessionStorage.setItem(autoSyncKey, '1');
    } catch {
      if (cacheIsFresh) return;
      void syncMarket({ showToast: false });
      return;
    }

    if (cacheIsFresh) return;
    void syncMarket({ showToast: false });
  }, [autoSyncKey, cacheKey, sources, syncMarket]);

  const skillTagFilter = useMemo<SkillTagFilterState>(() => {
    if (!enableTagFilter) return { audience: [], scenario: [] };
    const keyById = new Map([...tags.audienceTags, ...tags.scenarioTags].map((tag) => [tag.preset_tag_id, tag.key] as const));
    return {
      audience: tagFilter.audience.flatMap((id) => (keyById.has(id) ? [keyById.get(id)!] : [])),
      scenario: tagFilter.scenario.flatMap((id) => (keyById.has(id) ? [keyById.get(id)!] : [])),
    };
  }, [enableTagFilter, tagFilter, tags.audienceTags, tags.scenarioTags]);

  const filteredItems = useMemo(
    () => filterSkillMarketItems(items, activeSource, searchQuery, skillTagFilter),
    [activeSource, items, searchQuery, skillTagFilter],
  );

  const status = useMemo<MarketCatalogStatus>(() => {
    if (loading && items.length === 0) return 'loading';
    if (loading && items.length > 0) return 'cached-refresh';
    if (errors.length > 0 && items.length > 0) return 'partial-error';
    if (items.length > 0 && filteredItems.length === 0) return 'no-match';
    if (items.length === 0) return 'empty';
    return 'ready';
  }, [errors.length, filteredItems.length, items.length, loading]);

  const sourceCounts = useMemo(() => {
    const counts: Partial<Record<SkillMarketSource, number>> = {};
    for (const source of sources) counts[source] = 0;
    for (const item of items) if (sources.includes(item.source)) counts[item.source] = (counts[item.source] ?? 0) + 1;
    return counts;
  }, [items, sources]);

  return {
    activeSource,
    setActiveSource,
    items,
    fetchedAt,
    errors,
    loading,
    searchQuery,
    setSearchQuery: (value: string) => setSearchQuery(cleanMarketText(value, 80)),
    searchExpanded,
    setSearchExpanded,
    tagFilter,
    setTagFilter,
    filteredItems,
    status,
    sourceCounts,
    syncMarket,
    isSearchVisible: searchExpanded || searchQuery.length > 0,
    sourceLabel: marketSourceLabel(activeSource),
  };
};
