

import type { IConversationMcpStatusKind, TChatConversation, TokenUsageData } from '@/common/config/storage';
import { useSkillCatalog } from '@/renderer/hooks/skills/useSkillCatalog';
import { getConversationOrNull } from '@/renderer/pages/conversation/utils/conversationCache';
import { addEventListener } from '@/renderer/utils/emitter';
import { Empty } from '@arco-design/web-react';
import { ChartHistogram, Dashboard, Lightning, Time } from '@icon-park/react';
import React, { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  getMountedCapabilities,
  hasMountedCapabilities,
  resolveMountedSkillLabel,
  type MountedMcpChip,
} from './mountedCapabilities';
import {
  calculateCacheHitRatePercent,
  calculateContextUsagePercent,
  calculateContextUsageSegments,
  formatPercent,
  formatTokenCount,
  formatTurnDuration,
} from './turnMetrics';

const fallback = '—';

const formatFullNumber = (value?: number): string => (typeof value === 'number' ? value.toLocaleString() : fallback);
const formatCompactToken = (value?: number): string => (typeof value === 'number' ? formatTokenCount(value) : fallback);
// Catalog cost in USD, 4 significant digits ($0.0033); unknown price → “—”.
const formatMoaCost = (value: number | null): string =>
  typeof value === 'number' ? `$${Number(value.toPrecision(4))}` : fallback;

const getPersistedUsage = (conversation: TChatConversation): TokenUsageData | null =>
  ((conversation.extra as { last_token_usage?: TokenUsageData } | undefined)?.last_token_usage ?? null);

const hasUsageData = (usage: TokenUsageData | null): usage is TokenUsageData =>
  Boolean(
    usage &&
      ((usage.total_tokens ?? 0) > 0 ||
        (usage.input_tokens ?? 0) > 0 ||
        (usage.output_tokens ?? 0) > 0 ||
        (usage.context_tokens ?? 0) > 0 ||
        (usage.cache_read_tokens ?? 0) > 0 ||
        (usage.cache_creation_tokens ?? 0) > 0)
  );

const formatSessionSpan = (createdAt?: number, modifiedAt?: number): string => {
  if (!createdAt || !modifiedAt || modifiedAt <= createdAt) return fallback;
  return formatTurnDuration(modifiedAt - createdAt);
};

const getContextTone = (percent: number | null): { labelKey: string; color: string } => {
  if (percent == null) {
    return { labelKey: 'conversation.sessionMetrics.status.unknown', color: 'var(--color-text-3)' };
  }
  if (percent >= 90) {
    return { labelKey: 'conversation.sessionMetrics.status.tight', color: 'rgb(var(--danger-6))' };
  }
  if (percent >= 70) {
    return { labelKey: 'conversation.sessionMetrics.status.warming', color: 'rgb(var(--warning-6))' };
  }
  return { labelKey: 'conversation.sessionMetrics.status.healthy', color: 'rgb(var(--success-6))' };
};

const MetricTile: React.FC<{
  label: string;
  value: string;
  caption?: string;
  icon?: React.ReactNode;
}> = ({ label, value, caption, icon }) => {
  return (
    <div className='rounded-8px border border-solid border-[var(--color-border-2)] bg-fill-1 px-10px py-9px min-w-0'>
      <div className='flex items-center justify-between gap-8px text-[11px] text-t-tertiary leading-16px'>
        <span className='truncate'>{label}</span>
        {icon && <span className='shrink-0 text-t-tertiary'>{icon}</span>}
      </div>
      <div className='mt-4px text-18px leading-24px font-600 text-t-primary tabular-nums truncate'>{value}</div>
      {caption && <div className='mt-2px text-[11px] leading-15px text-t-tertiary truncate'>{caption}</div>}
    </div>
  );
};

const DistributionLegend: React.FC<{
  color: string;
  label: string;
  value: string;
  percent: number;
}> = ({ color, label, value, percent }) => (
  <div className='flex items-center justify-between gap-8px text-11px leading-16px'>
    <span className='flex items-center gap-6px min-w-0 text-t-secondary'>
      <span className='size-7px rounded-full shrink-0' style={{ background: color }} />
      <span className='truncate'>{label}</span>
    </span>
    <span className='tabular-nums text-t-tertiary shrink-0'>
      {value} · {percent}%
    </span>
  </div>
);

const mcpChipClassName = (status?: IConversationMcpStatusKind): string => {
  if (status == null) {
    return 'border-[var(--color-border-2)] bg-fill-2 text-t-secondary';
  }
  switch (status) {
    case 'loaded':
      return 'border-[rgba(var(--success-6),0.28)] bg-[rgba(var(--success-1),0.72)] text-[rgb(var(--success-7))]';
    case 'failed':
      return 'border-[rgba(var(--danger-6),0.28)] bg-[rgba(var(--danger-1),0.72)] text-[rgb(var(--danger-7))]';
    case 'unsupported':
      return 'border-[rgba(var(--warning-6),0.28)] bg-[rgba(var(--warning-1),0.72)] text-[rgb(var(--warning-7))]';
    default: {
      const _exhaustive: never = status;
      return _exhaustive;
    }
  }
};

const CapabilityChip: React.FC<{ label: string; title?: string; className?: string }> = ({
  label,
  title,
  className,
}) => (
  <span
    className={`inline-flex max-w-full items-center truncate rounded-full border border-solid px-8px py-2px text-11px leading-16px ${className ?? ''}`}
    title={title || label}
  >
    {label}
  </span>
);

const MountedChipRow: React.FC<{
  label: string;
  count: number;
  emptyLabel: string;
  children: React.ReactNode;
}> = ({ label, count, emptyLabel, children }) => (
  <div>
    <div className='flex items-center justify-between gap-8px'>
      <div className='text-11px text-t-tertiary leading-16px'>{label}</div>
      <div className='text-11px tabular-nums text-t-tertiary leading-16px'>{count}</div>
    </div>
    {count > 0 ? (
      <div className='mt-6px flex flex-wrap gap-6px'>{children}</div>
    ) : (
      <div className='mt-6px text-11px leading-16px text-t-tertiary'>{emptyLabel}</div>
    )}
  </div>
);

const MountedCapabilitiesSection: React.FC<{ conversation: TChatConversation }> = ({ conversation }) => {
  const { t } = useTranslation();
  const mounted = useMemo(() => getMountedCapabilities(conversation), [conversation]);
  const { skills: catalog } = useSkillCatalog(mounted.skills.length > 0);
  const emptyLabel = t('conversation.sessionMetrics.mountedEmpty');

  const mcpTitle = (chip: MountedMcpChip): string | undefined => {
    if (!chip.status) return undefined;
    const statusLabel = t(`conversation.mcp.status.${chip.status}` as const);
    return chip.reason ? `${statusLabel} · ${chip.reason}` : statusLabel;
  };

  return (
    <section
      className='mt-12px rounded-8px border border-solid border-[var(--color-border-2)] bg-fill-1 p-10px'
      data-testid='nomi-session-metrics-mounted'
    >
      <div className='mb-7px text-12px font-600 leading-18px'>{t('conversation.sessionMetrics.mountedTitle')}</div>
      <div className='max-h-180px overflow-y-auto space-y-10px'>
        <MountedChipRow
          label={t('conversation.sessionMetrics.mountedMcp')}
          count={mounted.mcp.length}
          emptyLabel={emptyLabel}
        >
          {mounted.mcp.map((chip) => (
            <CapabilityChip
              key={`${chip.id}-${chip.name}`}
              label={chip.name}
              title={mcpTitle(chip)}
              className={mcpChipClassName(chip.status)}
            />
          ))}
        </MountedChipRow>
        <MountedChipRow
          label={t('conversation.sessionMetrics.mountedSkills')}
          count={mounted.skills.length}
          emptyLabel={emptyLabel}
        >
          {mounted.skills.map((skillId) => {
            const resolved = resolveMountedSkillLabel(skillId, catalog);
            return (
              <CapabilityChip
                key={skillId}
                label={resolved.label}
                title={resolved.description}
                className='border-[var(--color-border-2)] bg-fill-2 text-t-secondary'
              />
            );
          })}
        </MountedChipRow>
      </div>
    </section>
  );
};

const NomiSessionMetricsPanel: React.FC<{ conversation: TChatConversation }> = ({ conversation }) => {
  const { t } = useTranslation();
  const [usage, setUsage] = useState<TokenUsageData | null>(() => getPersistedUsage(conversation));

  useEffect(() => {
    setUsage(getPersistedUsage(conversation));
  }, [conversation]);

  // Metrics tab content mounts only while selected, so live `nomi.usage.updated`
  // events during a turn are often missed. Re-read the conversation row on mount
  // (and when switching conversations) to pick up persisted last_token_usage.
  useEffect(() => {
    let cancelled = false;
    void getConversationOrNull(conversation.id).then((latest) => {
      if (cancelled || !latest) return;
      const persisted = getPersistedUsage(latest);
      if (hasUsageData(persisted)) {
        setUsage(persisted);
      }
    });
    return () => {
      cancelled = true;
    };
  }, [conversation.id]);

  useEffect(() => {
    return addEventListener('nomi.usage.updated', ({ conversation_id, tokenUsage }) => {
      if (conversation_id === conversation.id) {
        setUsage(tokenUsage);
      }
    });
  }, [conversation.id]);

  const contextPercent = calculateContextUsagePercent(usage?.context_tokens, usage?.context_window);
  const contextSegments = calculateContextUsageSegments({
    contextTokens: usage?.context_tokens,
    contextWindow: usage?.context_window,
    cacheReadTokens: usage?.cache_read_tokens,
  });
  const cachePercent = calculateCacheHitRatePercent({
    inputTokens: usage?.input_tokens,
    cacheReadTokens: usage?.cache_read_tokens,
  });
  const contextTone = getContextTone(contextPercent);
  const cachedContextColor = 'rgb(var(--primary-6))';
  const freshContextColor = contextTone.color;
  const remainingContextColor = 'var(--color-fill-3)';
  const sessionSpan = useMemo(
    () => formatSessionSpan(conversation.created_at, conversation.modified_at),
    [conversation.created_at, conversation.modified_at]
  );
  const mounted = useMemo(() => getMountedCapabilities(conversation), [conversation]);
  const usageData = hasUsageData(usage) ? usage : null;

  if (!usageData && !hasMountedCapabilities(mounted)) {
    return (
      <div className='size-full flex items-center justify-center px-16px'>
        <Empty
          description={
            <div className='text-center'>
              <div className='text-14px font-600 text-t-secondary'>
                {t('conversation.sessionMetrics.emptyTitle')}
              </div>
              <div className='mt-4px text-12px leading-18px text-t-tertiary'>
                {t('conversation.sessionMetrics.emptyDesc')}
              </div>
            </div>
          }
        />
      </div>
    );
  }

  return (
    <div className='w-full p-12px pb-16px box-border text-t-primary' data-testid='nomi-session-metrics-panel'>
      <div className='mb-12px'>
        <div className='text-13px font-600 leading-20px'>{t('conversation.sessionMetrics.title')}</div>
        <div className='text-11px text-t-tertiary leading-16px'>{t('conversation.sessionMetrics.subtitle')}</div>
        {usageData && (
          <div className='mt-8px rounded-6px border border-solid border-[rgb(var(--warning-3))] bg-[rgba(var(--warning-1),0.72)] px-8px py-6px text-11px leading-16px text-[rgb(var(--warning-8))]'>
            {t('conversation.sessionMetrics.notice')}
          </div>
        )}
      </div>

      {usageData && (
        <div className='grid grid-cols-2 gap-8px'>
          <MetricTile
            label={t('conversation.sessionMetrics.elapsed')}
            value={typeof usageData.elapsed_ms === 'number' ? formatTurnDuration(usageData.elapsed_ms) : fallback}
            caption={t('conversation.sessionMetrics.elapsedCaption')}
            icon={<Time theme='outline' size='14' />}
          />
          <MetricTile
            label={t('conversation.sessionMetrics.sessionSpan')}
            value={sessionSpan}
            caption={t('conversation.sessionMetrics.sessionSpanCaption')}
            icon={<Dashboard theme='outline' size='14' />}
          />
        </div>
      )}

      <MountedCapabilitiesSection conversation={conversation} />

      {usageData && (
        <>
          <section className='mt-12px rounded-8px border border-solid border-[var(--color-border-2)] bg-fill-1 p-10px'>
            <div className='flex items-center justify-between gap-8px'>
              <div>
                <div className='text-12px font-600 leading-18px'>{t('conversation.sessionMetrics.contextTitle')}</div>
                <div className='text-11px text-t-tertiary leading-16px'>
                  {formatFullNumber(usageData.context_tokens)} / {formatFullNumber(usageData.context_window)}
                </div>
              </div>
              <div className='text-right'>
                <div className='text-18px leading-24px font-600 tabular-nums' style={{ color: contextTone.color }}>
                  {formatPercent(contextPercent)}
                </div>
                <div className='text-11px text-t-tertiary leading-16px'>{t(contextTone.labelKey)}</div>
              </div>
            </div>
            {contextSegments && (
              <>
                <div className='mt-9px h-8px rounded-full bg-fill-3 overflow-hidden flex'>
                  {contextSegments.cachedTokens > 0 && (
                    <div
                      className='h-full transition-width duration-200'
                      style={{ width: `${contextSegments.cachedPercent}%`, background: cachedContextColor }}
                    />
                  )}
                  {contextSegments.freshTokens > 0 && (
                    <div
                      className='h-full transition-width duration-200'
                      style={{ width: `${contextSegments.freshPercent}%`, background: freshContextColor }}
                    />
                  )}
                  {contextSegments.remainingTokens > 0 && (
                    <div
                      className='h-full transition-width duration-200'
                      style={{ width: `${contextSegments.remainingPercent}%`, background: remainingContextColor }}
                    />
                  )}
                </div>
                <div className='mt-8px space-y-4px'>
                  <DistributionLegend
                    color={cachedContextColor}
                    label={t('conversation.sessionMetrics.contextCached')}
                    value={formatCompactToken(contextSegments.cachedTokens)}
                    percent={contextSegments.cachedPercent}
                  />
                  <DistributionLegend
                    color={freshContextColor}
                    label={t('conversation.sessionMetrics.contextFresh')}
                    value={formatCompactToken(contextSegments.freshTokens)}
                    percent={contextSegments.freshPercent}
                  />
                  <DistributionLegend
                    color={remainingContextColor}
                    label={t('conversation.sessionMetrics.contextRemaining')}
                    value={formatCompactToken(contextSegments.remainingTokens)}
                    percent={contextSegments.remainingPercent}
                  />
                </div>
              </>
            )}
          </section>

          <section className='mt-12px'>
            <div className='mb-7px text-12px font-600 leading-18px'>{t('conversation.sessionMetrics.tokensTitle')}</div>
            <div className='grid grid-cols-2 gap-8px'>
              <MetricTile
                label={t('conversation.sessionMetrics.totalTokens')}
                value={formatCompactToken(usageData.total_tokens)}
                caption={formatFullNumber(usageData.total_tokens)}
                icon={<ChartHistogram theme='outline' size='14' />}
              />
              <MetricTile
                label={t('conversation.sessionMetrics.inputTokens')}
                value={formatCompactToken(usageData.input_tokens)}
                caption={formatFullNumber(usageData.input_tokens)}
              />
              <MetricTile
                label={t('conversation.sessionMetrics.outputTokens')}
                value={formatCompactToken(usageData.output_tokens)}
                caption={formatFullNumber(usageData.output_tokens)}
              />
              <MetricTile
                label={t('conversation.sessionMetrics.cacheHitRate')}
                value={formatPercent(cachePercent)}
                caption={t('conversation.sessionMetrics.cacheHitCaption')}
                icon={<Lightning theme='outline' size='14' />}
              />
            </div>
          </section>

          <section className='mt-12px rounded-8px border border-solid border-[var(--color-border-2)] bg-fill-1 p-10px'>
            <div className='mb-7px text-12px font-600 leading-18px'>{t('conversation.sessionMetrics.cacheTitle')}</div>
            <div className='grid grid-cols-2 gap-8px'>
              <div>
                <div className='text-11px text-t-tertiary leading-16px'>
                  {t('conversation.sessionMetrics.cacheReadTokens')}
                </div>
                <div className='mt-2px text-15px font-600 tabular-nums'>
                  {formatFullNumber(usageData.cache_read_tokens)}
                </div>
              </div>
              <div>
                <div className='text-11px text-t-tertiary leading-16px'>
                  {t('conversation.sessionMetrics.cacheWriteTokens')}
                </div>
                <div className='mt-2px text-15px font-600 tabular-nums'>
                  {formatFullNumber(usageData.cache_creation_tokens)}
                </div>
              </div>
            </div>
          </section>

          {usageData.moa && usageData.moa.slots.length > 0 && (
            <section className='mt-12px rounded-8px border border-solid border-[var(--color-border-2)] bg-fill-1 p-10px'>
              <div className='mb-7px text-12px font-600 leading-18px'>{t('conversation.sessionMetrics.moaTitle')}</div>
              <div className='space-y-4px'>
                {usageData.moa.slots.map((slot, index) => (
                  <div
                    key={`${slot.label}-${index}`}
                    className='flex items-center justify-between gap-8px text-11px leading-16px'
                  >
                    <span className='truncate min-w-0 text-t-secondary'>{slot.label}</span>
                    <span className='tabular-nums text-t-tertiary shrink-0'>
                      {formatCompactToken(slot.input_tokens)}/{formatCompactToken(slot.output_tokens)} ·{' '}
                      {formatMoaCost(slot.cost_usd)}
                    </span>
                  </div>
                ))}
                <div className='flex items-center justify-between gap-8px text-11px leading-16px pt-4px border-t border-solid border-[var(--color-border-2)]'>
                  <span className='truncate min-w-0 font-600 text-t-primary'>
                    {t('conversation.sessionMetrics.moaTotal')}
                  </span>
                  <span className='tabular-nums font-600 text-t-primary shrink-0'>
                    {formatMoaCost(usageData.moa.total_cost_usd)}
                  </span>
                </div>
              </div>
            </section>
          )}
        </>
      )}
    </div>
  );
};

export default NomiSessionMetricsPanel;
