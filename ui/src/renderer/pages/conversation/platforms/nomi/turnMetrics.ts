/**
 * Pure formatters for the per-turn metrics chip (token cost + wall-clock
 * duration) shown after a nomi turn completes. Kept separate from React so the
 * formatting rules are unit-testable in isolation.
 */

/**
 * Compact token count: `942`, `1.2k`, `2.3m`. One decimal place at each
 * magnitude so the chip stays narrow while still conveying scale.
 */
export function formatTokenCount(tokens: number): string {
  if (tokens < 1000) {
    return String(tokens);
  }
  if (tokens < 1_000_000) {
    return `${(tokens / 1000).toFixed(1)}k`;
  }
  return `${(tokens / 1_000_000).toFixed(1)}m`;
}

/**
 * Cursor-style token abbrev for the context usage panel: `451`, `95.4K`, `272K`.
 * Whole thousands at or above 100K drop the decimal; smaller K/M keep one place.
 */
export function formatContextTokenAbbrev(tokens: number): string {
  const safe = Math.max(0, Math.round(tokens));
  if (safe < 1000) {
    return String(safe);
  }
  if (safe < 1_000_000) {
    const value = safe / 1000;
    return `${value >= 100 ? value.toFixed(0) : value.toFixed(1)}K`;
  }
  const value = safe / 1_000_000;
  return `${value >= 100 ? value.toFixed(0) : value.toFixed(1)}M`;
}

/**
 * Human wall-clock duration: `840ms`, `3.5s`, `1m 30s`. Sub-second in ms,
 * seconds with one decimal under a minute, `Xm Ys` at or above a minute.
 */
export function formatTurnDuration(elapsedMs: number): string {
  if (elapsedMs < 1000) {
    return `${elapsedMs}ms`;
  }
  if (elapsedMs < 60_000) {
    return `${(elapsedMs / 1000).toFixed(1)}s`;
  }
  const totalSeconds = Math.floor(elapsedMs / 1000);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}m ${seconds}s`;
}

export function calculateContextUsagePercent(used?: number, max?: number): number | null {
  if (used == null || max == null || max <= 0) {
    return null;
  }
  return Math.min(100, Math.max(0, Math.round((used / max) * 100)));
}

export function calculateCacheHitRatePercent({
  inputTokens,
  cacheReadTokens = 0,
}: {
  inputTokens?: number;
  cacheReadTokens?: number;
}): number | null {
  if (inputTokens == null || inputTokens <= 0) {
    return null;
  }
  return Math.max(0, Math.round((cacheReadTokens / inputTokens) * 100));
}

export function formatPercent(percent: number | null | undefined): string {
  return percent == null ? '—' : `${percent}%`;
}

export type ContextUsageSegments = {
  cachedTokens: number;
  freshTokens: number;
  remainingTokens: number;
  cachedPercent: number;
  freshPercent: number;
  remainingPercent: number;
};

export function calculateContextUsageSegments({
  contextTokens,
  contextWindow,
  cacheReadTokens = 0,
}: {
  contextTokens?: number;
  contextWindow?: number;
  cacheReadTokens?: number;
}): ContextUsageSegments | null {
  if (contextTokens == null || contextWindow == null || contextWindow <= 0) {
    return null;
  }

  const usedTokens = Math.min(Math.max(0, contextTokens), contextWindow);
  const cachedTokens = Math.min(Math.max(0, cacheReadTokens), usedTokens);
  const freshTokens = Math.max(0, usedTokens - cachedTokens);
  const remainingTokens = Math.max(0, contextWindow - usedTokens);

  return {
    cachedTokens,
    freshTokens,
    remainingTokens,
    cachedPercent: Math.round((cachedTokens / contextWindow) * 100),
    freshPercent: Math.round((freshTokens / contextWindow) * 100),
    remainingPercent: Math.round((remainingTokens / contextWindow) * 100),
  };
}

export const CONTEXT_USAGE_CATEGORY_ORDER = [
  'system_prompt',
  'tool_definitions',
  'rules',
  'skills',
  'mcp_and_dynamic_tools',
  'subagent_definitions',
  'summarized_conversation',
  'conversation',
] as const;

export type ContextUsageCategory = (typeof CONTEXT_USAGE_CATEGORY_ORDER)[number];

export const CONTEXT_USAGE_CATEGORY_COLORS: Record<ContextUsageCategory, string> = {
  system_prompt: '#9CA3AF',
  tool_definitions: '#8B5CF6',
  rules: '#059669',
  skills: '#B45309',
  mcp_and_dynamic_tools: '#DB2777',
  subagent_definitions: '#2563EB',
  summarized_conversation: '#E11D48',
  conversation: '#EF4444',
};

export type ContextBreakdownCategoryTokens = Partial<Record<ContextUsageCategory, number>>;

export type SummarizedConversationPropertiesView = {
  trigger?: 'auto' | 'manual' | null;
  pre_compact_tokens?: number | null;
  messages_summarized?: number | null;
};

export type ContextBreakdownSegment = {
  key: ContextUsageCategory | 'cached' | 'fresh' | 'used' | 'remaining';
  tokens: number;
  percentOfWindow: number;
  color: string;
  category?: ContextUsageCategory;
};

export type ContextBreakdownViewModel = {
  pctFull: number;
  usedText: string;
  maxText: string;
  tone: string;
  mode: 'categories' | 'legacy';
  listSegments: ContextBreakdownSegment[];
  barSegments: ContextBreakdownSegment[];
  remainingTokens: number;
  summarizedProps?: SummarizedConversationPropertiesView;
};

function getUsageTone(pct: number): string {
  if (pct >= 90) return 'rgb(var(--danger-6))';
  if (pct >= 70) return 'rgb(var(--warning-6))';
  return 'rgb(var(--success-6))';
}

function readCategoryTokens(breakdown: ContextBreakdownCategoryTokens, key: ContextUsageCategory): number {
  const value = breakdown[key];
  return typeof value === 'number' && Number.isFinite(value) ? Math.max(0, Math.round(value)) : 0;
}

export function hasContextBreakdownCategories(
  breakdown?: ContextBreakdownCategoryTokens | null
): breakdown is ContextBreakdownCategoryTokens {
  if (!breakdown) return false;
  return CONTEXT_USAGE_CATEGORY_ORDER.some((key) => readCategoryTokens(breakdown, key) > 0);
}

export function buildContextBreakdownViewModel({
  used,
  max,
  cacheReadTokens,
  breakdown,
  summarized,
}: {
  used?: number;
  max?: number;
  cacheReadTokens?: number;
  breakdown?: ContextBreakdownCategoryTokens | null;
  summarized?: SummarizedConversationPropertiesView | null;
}): ContextBreakdownViewModel | null {
  if (used == null || max == null || max <= 0) {
    return null;
  }

  const usedTokens = Math.min(Math.max(0, used), max);
  const pctFull = Math.min(100, Math.max(0, Math.round((usedTokens / max) * 100)));
  const tone = getUsageTone(pctFull);
  const remainingTokens = Math.max(0, max - usedTokens);
  const remainingPercent = Math.round((remainingTokens / max) * 100);

  if (hasContextBreakdownCategories(breakdown)) {
    const listSegments: ContextBreakdownSegment[] = [];
    for (const key of CONTEXT_USAGE_CATEGORY_ORDER) {
      const tokens = readCategoryTokens(breakdown, key);
      if (tokens <= 0) continue;
      listSegments.push({
        key,
        category: key,
        tokens,
        percentOfWindow: Math.round((tokens / max) * 100),
        color: CONTEXT_USAGE_CATEGORY_COLORS[key],
      });
    }

    const barSegments: ContextBreakdownSegment[] = [...listSegments];
    if (remainingTokens > 0) {
      barSegments.push({
        key: 'remaining',
        tokens: remainingTokens,
        percentOfWindow: remainingPercent,
        color: 'var(--color-fill-3)',
      });
    }

    return {
      pctFull,
      usedText: formatContextTokenAbbrev(usedTokens),
      maxText: formatContextTokenAbbrev(max),
      tone,
      mode: 'categories',
      listSegments,
      barSegments,
      remainingTokens,
      summarizedProps: summarized ?? undefined,
    };
  }

  const legacy = calculateContextUsageSegments({
    contextTokens: usedTokens,
    contextWindow: max,
    cacheReadTokens,
  });
  if (!legacy) return null;

  const listSegments: ContextBreakdownSegment[] = [];
  if (legacy.cachedTokens > 0) {
    listSegments.push({
      key: 'cached',
      tokens: legacy.cachedTokens,
      percentOfWindow: legacy.cachedPercent,
      color: 'rgb(var(--primary-6))',
    });
  }
  if (legacy.freshTokens > 0) {
    listSegments.push({
      key: 'fresh',
      tokens: legacy.freshTokens,
      percentOfWindow: legacy.freshPercent,
      color: tone,
    });
  }
  if (listSegments.length === 0 && usedTokens > 0) {
    listSegments.push({
      key: 'used',
      tokens: usedTokens,
      percentOfWindow: pctFull,
      color: tone,
    });
  }

  const barSegments: ContextBreakdownSegment[] = [...listSegments];
  if (legacy.remainingTokens > 0) {
    barSegments.push({
      key: 'remaining',
      tokens: legacy.remainingTokens,
      percentOfWindow: legacy.remainingPercent,
      color: 'var(--color-fill-3)',
    });
  }

  return {
    pctFull,
    usedText: formatContextTokenAbbrev(usedTokens),
    maxText: formatContextTokenAbbrev(max),
    tone,
    mode: 'legacy',
    listSegments,
    barSegments,
    remainingTokens: legacy.remainingTokens,
  };
}
