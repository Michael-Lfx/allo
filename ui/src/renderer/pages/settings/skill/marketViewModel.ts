import type {
  ISkillHubMarketItem,
  ISkillMarketItem,
  SkillHubMarketContentSource,
  SkillMarketSource,
} from '@/common/adapter/ipcBridge';
import { cleanMarketText, translateMarketDescription } from './skillMarket';

/**
 * The renderer-facing shape of an item in any of the capability markets.
 *
 * Keeping the raw DTO on the view model is intentional: actions still pass the
 * exact server-validated item to the existing business flow, while rendering
 * code only deals with already-localized, bounded display values.
 */
export type MarketItemViewModel = {
  raw: ISkillMarketItem;
  id: string;
  rank: number | null;
  title: string;
  source: SkillMarketSource;
  marketSource: SkillHubMarketContentSource;
  upstreamSource?: string | null;
  summary: string;
  compactStats?: string;
  fullDescription: string;
  visibleTags: string[];
  allTags: string[];
  overflowTagCount: number;
  fullStats?: string;
  installCommand: string;
  sourceUrl: string;
  requiresApi: boolean;
  noApi: boolean;
  apiKeyUnknown: boolean;
  avatar?: string;
  skillHub?: {
    owner: string;
    slug: string;
    version: string;
    category?: string | null;
    subCategories: Array<{ key: string; name: string }>;
    tags: string[];
    requiresApiKey: boolean | null;
    downloads: number;
    installs: number;
    stars: number;
    score: number;
    createdAt?: number | null;
    updatedAt?: number | null;
  };
};

type MarketTag = {
  label?: string;
  label_i18n?: Record<string, string | undefined>;
};

type Translate = (key: string, options?: Record<string, unknown>) => string;

const MAX_VISIBLE_TAGS = 2;

/**
 * Keep large SkillHub counters scannable without hiding the exact value from
 * the detail view. SkillHub's counters are integers, so rounding here also
 * protects the UI from an unexpected fractional upstream value.
 */
export const formatSkillHubMarketCount = (value: number): string => {
  const safe = Number.isFinite(value) ? Math.max(0, Math.round(value)) : 0;
  if (safe < 1000) return String(safe);

  const compact = (safe / 1000).toFixed(1).replace(/\.0$/, '');
  return `${compact}k`;
};

const resolveTagLabel = (key: string, localeKey: string, tagByKey: ReadonlyMap<string, MarketTag>): string => {
  const tag = tagByKey.get(key);
  return tag?.label_i18n?.[localeKey] || tag?.label || key;
};

const localizeStats = (stats: string | undefined, t: Translate): { compact?: string; full?: string } => {
  const value = stats?.trim();
  if (!value) return {};

  const parts = value.split(/[·•]/).map((part) => part.trim()).filter(Boolean);
  const localized = parts.map((part) => {
    const match = part.match(/^(\d+)\s+(skills?|downloads?|installs?|stars?)$/i);
    if (!match) return part;
    const [, count, unit] = match;
    const keyByUnit = {
      skill: 'settings.market.skillsCount',
      skills: 'settings.market.skillsCount',
      download: 'settings.market.downloadsCount',
      downloads: 'settings.market.downloadsCount',
      install: 'settings.market.installsCount',
      installs: 'settings.market.installsCount',
      star: 'settings.market.starsCount',
      stars: 'settings.market.starsCount',
    } as const;
    return t(keyByUnit[unit.toLowerCase() as keyof typeof keyByUnit], {
      count: Number(count),
      defaultValue: part,
    });
  });
  const full = localized.join(' · ');
  const allZero = parts.every((part) => /^0\b/.test(part));
  return { compact: allZero ? undefined : full, full };
};

export const createMarketItemViewModel = (
  item: ISkillMarketItem,
  options: {
    localeKey: string;
    tagByKey?: ReadonlyMap<string, MarketTag>;
    t: Translate;
  },
): MarketItemViewModel => {
  const tagByKey = options.tagByKey ?? new Map<string, MarketTag>();
  const semanticKeys = [...(item.audience_tags ?? []), ...(item.scenario_tags ?? [])];
  const technicalKeys = (item.tags ?? []).filter(
    (tag) => tag !== 'requires_api_key' && tag !== 'no_api_key',
  );
  const allKeys = [...semanticKeys, ...technicalKeys].filter((tag, index, list) => list.indexOf(tag) === index);
  const allTags = allKeys.map((key) => resolveTagLabel(key, options.localeKey, tagByKey));
  const stats = localizeStats(item.stats, options.t);

  return {
    raw: item,
    id: item.id,
    rank: item.rank > 0 ? item.rank : null,
    title: item.name,
    source: item.source,
    marketSource: item.source === 'skillhub' ? 'skillhub' : 'unknown',
    upstreamSource: null,
    summary: translateMarketDescription(item.description, item, options.localeKey),
    compactStats: stats.compact,
    fullDescription:
      translateMarketDescription(item.description, item, options.localeKey) ||
      options.t('settings.skillsMarket.noDescription', { defaultValue: '暂无描述。' }),
    visibleTags: allTags.slice(0, MAX_VISIBLE_TAGS),
    allTags,
    overflowTagCount: Math.max(0, allTags.length - MAX_VISIBLE_TAGS),
    fullStats: stats.full,
    installCommand: item.install_command,
    sourceUrl: item.url,
    requiresApi: item.tags?.includes('requires_api_key') ?? false,
    noApi: item.tags?.includes('no_api_key') ?? false,
    apiKeyUnknown: false,
    avatar: item.avatar,
  };
};

/**
 * Adapt the structured SkillHub DTO to the shared card/action shell without
 * flattening metadata or reintroducing a user-visible install command.
 */
export const createSkillHubMarketItemViewModel = (
  item: ISkillHubMarketItem,
  options: {
    localeKey: string;
    t: Translate;
    categoryLabel?: (key: string) => string;
  },
): MarketItemViewModel => {
  const raw: ISkillMarketItem = {
    id: item.id,
    source: 'skillhub',
    rank: item.rank,
    name: cleanMarketText(item.name, 96),
    description: cleanMarketText(item.description, 8192),
    url: item.url,
    install_command: '',
    install_mode: 'native',
    tags: item.tags,
    avatar: item.avatar ?? undefined,
    market_source: item.market_source,
    upstream_source: item.upstream_source,
  };
  const model = createMarketItemViewModel(raw, options);
  const fullDescription = cleanMarketText(item.description, 8192);
  return {
    ...model,
    marketSource: item.market_source,
    upstreamSource: item.upstream_source,
    summary: cleanMarketText(item.description),
    fullDescription:
      fullDescription || options.t('settings.skillsMarket.noDescription', { defaultValue: '暂无描述。' }),
    requiresApi: item.requires_api_key === true,
    noApi: item.requires_api_key === false,
    apiKeyUnknown: item.requires_api_key === null,
    skillHub: {
      owner: item.owner,
      slug: item.slug,
      version: item.version,
      category: item.category ? options.categoryLabel?.(item.category) ?? item.category : item.category,
      subCategories: item.sub_categories,
      tags: item.tags,
      requiresApiKey: item.requires_api_key,
      downloads: item.downloads,
      installs: item.installs,
      stars: item.stars,
      score: item.score,
      createdAt: item.created_at,
      updatedAt: item.updated_at,
    },
  };
};

export const marketViewModelTestables = { localizeStats };
