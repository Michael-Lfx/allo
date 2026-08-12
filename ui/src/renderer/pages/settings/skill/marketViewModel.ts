import type { ISkillMarketItem, SkillMarketSource } from '@/common/adapter/ipcBridge';
import { translateMarketDescription } from './skillMarket';

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
};

type MarketTag = {
  label?: string;
  label_i18n?: Record<string, string | undefined>;
};

type Translate = (key: string, options?: Record<string, unknown>) => string;

const MAX_VISIBLE_TAGS = 2;

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
  };
};

export const marketViewModelTestables = { localizeStats };
