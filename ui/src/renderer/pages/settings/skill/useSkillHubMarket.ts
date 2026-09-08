import type {
  ISkillHubMarketCategoriesResponse,
  ISkillHubMarketCategory,
  ISkillHubMarketItem,
  ISkillHubMarketQueryResponse,
  ISkillHubMarketQueryRequest,
  SkillHubMarketSource,
  SkillHubMarketSort,
} from '@/common/adapter/ipcBridge';
import { ipcBridge } from '@/common';
import { isSafeMarketAvatarUrl } from './skillMarket';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

const MARKET_CACHE_VERSION = 'v7';
const MARKET_CACHE_PREFIX = `nomifun.skillHubMarket.${MARKET_CACHE_VERSION}`;
const MARKET_CATEGORIES_CACHE_KEY = `${MARKET_CACHE_PREFIX}.categories`;
const DEFAULT_PAGE_SIZE = 20;

export type SkillHubMarketStatus = 'loading' | 'ready' | 'empty' | 'no-match' | 'error' | 'stale' | 'partial-error';
export type SkillHubMarketInstallationStatus = 'idle' | 'installing' | 'installed';
export type SkillHubMarketSourceFilter = 'all' | SkillHubMarketSource;

export const defaultSkillHubMarketSource = (language: string | undefined): SkillHubMarketSource =>
  language?.toLowerCase().startsWith('zh') ? 'skillhub' : 'clawhub';

type SkillHubMarketCacheEntry = {
  cached_at: number;
  response: ISkillHubMarketQueryResponse;
};

type SkillHubCategoriesCacheEntry = {
  cached_at: number;
  response: ISkillHubMarketCategoriesResponse;
};

export type SkillHubMarketQueryState = Pick<
  ISkillHubMarketQueryRequest,
  'category' | 'requires_api_key' | 'sort_by' | 'page_size'
> & {
  keyword: string;
  source?: SkillHubMarketSourceFilter;
};

const isFiniteNumber = (value: unknown): value is number =>
  typeof value === 'number' && Number.isFinite(value);

const isMarketSlug = (value: unknown): value is string =>
  typeof value === 'string' &&
  value.length >= 1 &&
  value.length <= 96 &&
  /^[A-Za-z0-9](?:[A-Za-z0-9._-]{0,94}[A-Za-z0-9])?$/.test(value) &&
  !value.includes('..');

const isNullableFiniteNumber = (value: unknown): value is number | null | undefined =>
  value === null || value === undefined || isFiniteNumber(value);

export const isSkillHubMarketItem = (value: unknown): value is ISkillHubMarketItem => {
  if (!value || typeof value !== 'object') return false;
  const item = value as Partial<ISkillHubMarketItem>;
  return (
    isMarketSlug(item.owner) &&
    isMarketSlug(item.slug) &&
    item.id === `skillhub:${item.owner}/skills/${item.slug}` &&
    (item.market_source === 'skillhub' || item.market_source === 'clawhub' || item.market_source === 'unknown') &&
    (item.upstream_source === null || item.upstream_source === undefined ||
      (typeof item.upstream_source === 'string' && item.upstream_source.length <= 64)) &&
    typeof item.name === 'string' &&
    item.name.length > 0 &&
    typeof item.description === 'string' &&
    typeof item.version === 'string' &&
    item.version.length > 0 &&
    (typeof item.category === 'string' || item.category === null || item.category === undefined) &&
    Array.isArray(item.tags) &&
    item.tags.every((tag) => typeof tag === 'string') &&
    Array.isArray(item.sub_categories) &&
    item.sub_categories.every(
      (category) =>
        Boolean(category) &&
        typeof category === 'object' &&
        typeof category.key === 'string' &&
        typeof category.name === 'string',
    ) &&
    (item.requires_api_key === null || typeof item.requires_api_key === 'boolean') &&
    isFiniteNumber(item.rank) &&
    item.rank >= 1 &&
    isFiniteNumber(item.downloads) &&
    item.downloads >= 0 &&
    isFiniteNumber(item.installs) &&
    item.installs >= 0 &&
    isFiniteNumber(item.stars) &&
    item.stars >= 0 &&
    isFiniteNumber(item.score) &&
    item.score >= 0 &&
    isNullableFiniteNumber(item.created_at) &&
    isNullableFiniteNumber(item.updated_at) &&
    item.url === `https://skillhub.cn/skills/${item.owner}/${item.slug}` &&
    (item.avatar === null || item.avatar === undefined ||
      (typeof item.avatar === 'string' && isSafeMarketAvatarUrl(item.avatar)))
  );
};

export const isSkillHubMarketResponse = (value: unknown): value is ISkillHubMarketQueryResponse => {
  if (!value || typeof value !== 'object') return false;
  const response = value as Partial<ISkillHubMarketQueryResponse>;
  return (
    isFiniteNumber(response.fetched_at) &&
    isFiniteNumber(response.total) &&
    isFiniteNumber(response.page) &&
    isFiniteNumber(response.page_size) &&
    response.page >= 1 &&
    response.page_size >= 1 &&
    Array.isArray(response.items) &&
    response.items.every(isSkillHubMarketItem)
  );
};

const isSkillHubCategoriesResponse = (value: unknown): value is ISkillHubMarketCategoriesResponse => {
  if (!value || typeof value !== 'object') return false;
  const response = value as Partial<ISkillHubMarketCategoriesResponse>;
  return (
    isFiniteNumber(response.fetched_at) &&
    Array.isArray(response.items) &&
    response.items.every(
      (item) =>
        Boolean(item) &&
        typeof item === 'object' &&
        typeof item.key === 'string' &&
        typeof item.name === 'string' &&
        typeof item.name_en === 'string' &&
        isFiniteNumber(item.sort_order),
    )
  );
};

const readLocalStorage = (key: string): unknown => {
  if (typeof window === 'undefined') return undefined;
  try {
    const raw = window.localStorage.getItem(key);
    return raw ? JSON.parse(raw) : undefined;
  } catch {
    return undefined;
  }
};

const writeLocalStorage = (key: string, value: unknown): void => {
  if (typeof window === 'undefined') return;
  try {
    window.localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // A full or disabled localStorage must not prevent the market from loading.
  }
};

export const skillHubMarketCacheKey = (query: SkillHubMarketQueryState, page: number): string => {
  const stableQuery = {
    keyword: query.keyword,
    source: query.source ?? 'all',
    category: query.category ?? null,
    requires_api_key: query.requires_api_key ?? null,
    sort_by: query.sort_by ?? 'score',
    page,
    page_size: query.page_size ?? DEFAULT_PAGE_SIZE,
  };
  return `${MARKET_CACHE_PREFIX}.query:${JSON.stringify(stableQuery)}`;
};

const readQueryCache = (key: string): SkillHubMarketCacheEntry | null => {
  const value = readLocalStorage(key);
  if (!value || typeof value !== 'object') return null;
  const entry = value as Partial<SkillHubMarketCacheEntry>;
  if (!isFiniteNumber(entry.cached_at) || !isSkillHubMarketResponse(entry.response)) return null;
  return { cached_at: entry.cached_at, response: entry.response };
};

const readCategoriesCache = (): SkillHubCategoriesCacheEntry | null => {
  const value = readLocalStorage(MARKET_CATEGORIES_CACHE_KEY);
  if (!value || typeof value !== 'object') return null;
  const entry = value as Partial<SkillHubCategoriesCacheEntry>;
  if (!isFiniteNumber(entry.cached_at) || !isSkillHubCategoriesResponse(entry.response)) return null;
  return { cached_at: entry.cached_at, response: entry.response };
};

const writeQueryCache = (key: string, response: ISkillHubMarketQueryResponse): void => {
  writeLocalStorage(key, { cached_at: Date.now(), response } satisfies SkillHubMarketCacheEntry);
};

const mergeUniqueItems = (current: ISkillHubMarketItem[], next: ISkillHubMarketItem[]): ISkillHubMarketItem[] => {
  const seen = new Set<string>();
  return [...current, ...next].filter((item) => {
    if (seen.has(item.id)) return false;
    seen.add(item.id);
    return true;
  });
};

export const mergeSkillHubMarketItems = mergeUniqueItems;

const errorMessage = (error: unknown): string => (error instanceof Error ? error.message : String(error));

export const useSkillHubMarket = ({
  enabled = true,
  defaultSource = 'skillhub',
}: {
  enabled?: boolean;
  defaultSource?: SkillHubMarketSourceFilter;
} = {}) => {
  const [searchQuery, setSearchQuery] = useState('');
  const [keyword, setKeyword] = useState('');
  const [category, setCategory] = useState<string | undefined>();
  const [requiresApiKey, setRequiresApiKey] = useState<boolean | undefined>();
  const [sortBy, setSortBy] = useState<SkillHubMarketSort>('score');
  const [source, setSource] = useState<SkillHubMarketSourceFilter>(defaultSource);
  const [items, setItems] = useState<ISkillHubMarketItem[]>([]);
  const [total, setTotal] = useState(0);
  const [page, setPage] = useState(0);
  const [fetchedAt, setFetchedAt] = useState<number | null>(null);
  const [status, setStatus] = useState<SkillHubMarketStatus>(enabled ? 'loading' : 'ready');
  const [error, setError] = useState<string | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);
  const [categories, setCategories] = useState<ISkillHubMarketCategory[]>([]);
  const [categoriesLoading, setCategoriesLoading] = useState(false);
  const [categoriesError, setCategoriesError] = useState<string | null>(null);
  const [installationStatus, setInstallationStatus] = useState<SkillHubMarketInstallationStatus>('idle');
  const [installingItemId, setInstallingItemId] = useState<string | null>(null);
  const requestSequenceRef = useRef(0);
  const categoriesSequenceRef = useRef(0);
  const queryKey = useMemo(
    () => ({
      keyword,
      source,
      category,
      requires_api_key: requiresApiKey,
      sort_by: sortBy,
      page_size: DEFAULT_PAGE_SIZE,
    }),
    [category, keyword, requiresApiKey, sortBy, source],
  );

  // Source choice is intentionally page-session state. When the market panel
  // is inactive/unmounted, reset to the locale-derived default so a later
  // activation can follow the current language again without localStorage
  // leaking a previous user's preference.
  useEffect(() => {
    if (!enabled) setSource(defaultSource);
  }, [defaultSource, enabled]);

  useEffect(() => {
    if (typeof window === 'undefined') {
      setKeyword(searchQuery.trim().slice(0, 80));
      return;
    }
    const timer = window.setTimeout(() => setKeyword(searchQuery.trim().slice(0, 80)), 300);
    return () => window.clearTimeout(timer);
  }, [searchQuery]);

  const loadCategories = useCallback(async () => {
    if (!enabled) return;
    const sequence = ++categoriesSequenceRef.current;
    setCategoriesLoading(true);
    setCategoriesError(null);
    const cached = readCategoriesCache();
    if (cached) setCategories(cached.response.items);
    try {
      const response = await ipcBridge.fs.listSkillHubMarketCategories.invoke();
      if (isSkillHubCategoriesResponse(response)) {
        if (sequence !== categoriesSequenceRef.current) return;
        setCategories(response.items);
        writeLocalStorage(MARKET_CATEGORIES_CACHE_KEY, {
          cached_at: Date.now(),
          response,
        } satisfies SkillHubCategoriesCacheEntry);
      } else {
        throw new Error('invalid SkillHub categories response');
      }
    } catch (loadError) {
      if (sequence !== categoriesSequenceRef.current) return;
      setCategoriesError(errorMessage(loadError));
      if (!cached) setCategories([]);
    } finally {
      if (sequence === categoriesSequenceRef.current) setCategoriesLoading(false);
    }
  }, [enabled]);

  useEffect(() => {
    if (!enabled) {
      ++categoriesSequenceRef.current;
      return;
    }
    void loadCategories();
    return () => {
      ++categoriesSequenceRef.current;
    };
  }, [enabled, loadCategories]);

  const loadPage = useCallback(
    async (targetPage: number, append: boolean) => {
      if (!enabled) return;
      const sequence = ++requestSequenceRef.current;
      const requestQuery: ISkillHubMarketQueryRequest = {
        keyword: queryKey.keyword || undefined,
        source: queryKey.source === 'all' ? undefined : queryKey.source,
        category: queryKey.category,
        requires_api_key: queryKey.requires_api_key,
        sort_by: queryKey.sort_by,
        page: targetPage,
        page_size: queryKey.page_size,
      };
      const cacheKey = skillHubMarketCacheKey(queryKey, targetPage);
      const cached = readQueryCache(cacheKey);

      if (!append) {
        setLoadingMore(false);
        setItems(cached?.response.items ?? []);
        setTotal(cached?.response.total ?? 0);
        setPage(cached?.response.page ?? 0);
        setFetchedAt(cached?.response.fetched_at ?? null);
        setStatus('loading');
        setError(null);
      } else {
        setLoadingMore(true);
        if (cached) {
          setItems((current) => mergeUniqueItems(current, cached.response.items));
          setTotal(cached.response.total);
          setPage(cached.response.page);
          setFetchedAt(cached.response.fetched_at);
        }
      }

      try {
        const response = await ipcBridge.fs.querySkillHubMarket.invoke(requestQuery);
        if (!isSkillHubMarketResponse(response)) throw new Error('invalid SkillHub market response');
        if (sequence !== requestSequenceRef.current) return;
        writeQueryCache(cacheKey, response);
        setTotal(response.total);
        setPage(response.page);
        setFetchedAt(response.fetched_at);
        setError(null);
        if (append) {
          setItems((current) => mergeUniqueItems(current, response.items));
        } else {
          // A successful empty page is authoritative. Never retain the cache
          // or results from a previous query in this state.
          setItems(response.items);
        }
        setStatus(response.items.length === 0 && !append && response.total === 0
          ? (queryKey.keyword ? 'no-match' : 'empty')
          : 'ready');
      } catch (loadError) {
        if (sequence !== requestSequenceRef.current) return;
        setError(errorMessage(loadError));
        if (cached) {
          setItems((current) => append
            ? mergeUniqueItems(current, cached.response.items)
            : cached.response.items);
          setTotal(cached.response.total);
          setPage(cached.response.page);
          setFetchedAt(cached.response.fetched_at);
          setStatus('stale');
        } else if (!append) {
          setItems([]);
          setTotal(0);
          setPage(0);
          setFetchedAt(null);
          setStatus('error');
        } else {
          setStatus(append ? 'partial-error' : 'stale');
        }
      } finally {
        if (sequence === requestSequenceRef.current) setLoadingMore(false);
      }
    },
    [enabled, queryKey],
  );

  useEffect(() => {
    if (!enabled) {
      ++requestSequenceRef.current;
      setItems([]);
      setTotal(0);
      setPage(0);
      setFetchedAt(null);
      setError(null);
      setStatus('ready');
      return;
    }
    void loadPage(1, false);
    return () => {
      // Invalidate the previous request before the next filter effect starts;
      // an old response must never repopulate a newer query.
      ++requestSequenceRef.current;
    };
  }, [enabled, keyword, category, requiresApiKey, sortBy, source, loadPage]);

  const refresh = useCallback(() => loadPage(1, false), [loadPage]);
  const loadMore = useCallback(() => {
    if (status === 'loading' || loadingMore || page < 1 || page * (queryKey.page_size ?? DEFAULT_PAGE_SIZE) >= total) return;
    return loadPage(page + 1, true);
  }, [loadPage, loadingMore, page, queryKey.page_size, status, total]);

  const beginInstallation = useCallback((itemId: string) => {
    setInstallingItemId(itemId);
    setInstallationStatus('installing');
  }, []);
  const markInstalled = useCallback(() => {
    setInstallingItemId(null);
    setInstallationStatus('installed');
  }, []);
  const clearInstallationStatus = useCallback(() => {
    setInstallingItemId(null);
    setInstallationStatus('idle');
  }, []);

  return {
    items,
    total,
    page,
    pageSize: queryKey.page_size ?? DEFAULT_PAGE_SIZE,
    hasMore: page > 0 && page * (queryKey.page_size ?? DEFAULT_PAGE_SIZE) < total,
    fetchedAt,
    status,
    dataStatus: status,
    installationStatus,
    installingItemId,
    loading: status === 'loading',
    loadingMore,
    error,
    searchQuery,
    setSearchQuery,
    keyword,
    category,
    setCategory,
    source,
    setSource,
    requiresApiKey,
    setRequiresApiKey,
    sortBy,
    setSortBy,
    refresh,
    loadMore,
    beginInstallation,
    markInstalled,
    clearInstallationStatus,
    categories,
    categoriesLoading,
    categoriesError,
    retryCategories: loadCategories,
  };
};
