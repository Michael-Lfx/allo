
import type { ContextBreakdownData, SummarizedConversationProperties } from '@/common/config/storage';
import { Popover } from '@arco-design/web-react';
import { Close, Down, Up } from '@icon-park/react';
import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import {
  buildContextBreakdownViewModel,
  formatContextTokenAbbrev,
  type ContextBreakdownSegment,
  type ContextUsageCategory,
} from './turnMetrics';

export type ContextUsageRingProps = {
  used?: number;
  max?: number;
  cacheReadTokens?: number;
  breakdown?: ContextBreakdownData | null;
};

const CATEGORY_LABEL_KEYS: Record<ContextUsageCategory, string> = {
  system_prompt: 'conversation.contextUsage.categories.systemPrompt',
  tool_definitions: 'conversation.contextUsage.categories.toolDefinitions',
  rules: 'conversation.contextUsage.categories.rules',
  skills: 'conversation.contextUsage.categories.skills',
  mcp_and_dynamic_tools: 'conversation.contextUsage.categories.mcpDynamicTools',
  delegate_definitions: 'conversation.contextUsage.categories.delegateDefinitions',
  summarized_conversation: 'conversation.contextUsage.categories.summarizedConversation',
  conversation: 'conversation.contextUsage.categories.conversation',
};

const CATEGORY_DEFAULTS: Record<ContextUsageCategory, string> = {
  system_prompt: 'System prompt',
  tool_definitions: 'Tool definitions',
  rules: 'Rules',
  skills: 'Skills',
  mcp_and_dynamic_tools: 'MCP & dynamic tools',
  delegate_definitions: 'Delegate definitions',
  summarized_conversation: 'Summarized conversation',
  conversation: 'Conversation',
};

function segmentLabel(
  segment: ContextBreakdownSegment,
  t: (key: string, options?: Record<string, unknown>) => string
): string {
  if (segment.category) {
    return t(CATEGORY_LABEL_KEYS[segment.category], { defaultValue: CATEGORY_DEFAULTS[segment.category] });
  }
  const legacyKey = segment.key;
  switch (legacyKey) {
    case 'cached':
      return t('conversation.contextUsage.cached', { defaultValue: 'Cache hit' });
    case 'fresh':
      return t('conversation.contextUsage.fresh', { defaultValue: 'Conversation' });
    case 'used':
      return t('conversation.contextUsage.used', { defaultValue: 'Context' });
    case 'remaining':
      return t('conversation.contextUsage.remaining', { defaultValue: 'Remaining' });
    case 'system_prompt':
    case 'tool_definitions':
    case 'rules':
    case 'skills':
    case 'mcp_and_dynamic_tools':
    case 'delegate_definitions':
    case 'summarized_conversation':
    case 'conversation':
      return t(CATEGORY_LABEL_KEYS[legacyKey], { defaultValue: CATEGORY_DEFAULTS[legacyKey] });
    default: {
      const _exhaustive: never = legacyKey;
      return _exhaustive;
    }
  }
}

function hasSummarizedProps(props?: SummarizedConversationProperties | null): boolean {
  if (!props) return false;
  return (
    props.trigger != null ||
    typeof props.pre_compact_tokens === 'number' ||
    typeof props.messages_summarized === 'number'
  );
}

/** Icon-only context gauge shown beside the active model. The ring carries the
 * compact status; click opens a Cursor-style context usage breakdown. */
export function ContextUsageRing({ used, max, cacheReadTokens, breakdown }: ContextUsageRingProps) {
  const { t } = useTranslation();
  const [visible, setVisible] = useState(false);
  const [summarizedOpen, setSummarizedOpen] = useState(false);

  const panel = useMemo(
    () =>
      buildContextBreakdownViewModel({
        used,
        max,
        cacheReadTokens,
        breakdown,
        summarized: breakdown?.summarized,
      }),
    [breakdown, cacheReadTokens, max, used]
  );

  if (!panel) return null;

  const { pctFull, tone, listSegments, barSegments, usedText, maxText, summarizedProps, mode } = panel;
  const ringTrack = 'color-mix(in srgb, var(--color-fill-3) 76%, transparent)';
  const ringFill = `conic-gradient(${tone} ${pctFull * 3.6}deg, ${ringTrack} 0deg)`;
  const ariaLabel = t('conversation.contextUsage.tooltip', {
    used: (used ?? 0).toLocaleString(),
    max: (max ?? 0).toLocaleString(),
    pct: pctFull,
  });
  const canExpandSummarized =
    mode === 'categories' &&
    listSegments.some((segment) => segment.key === 'summarized_conversation') &&
    hasSummarizedProps(summarizedProps);

  const content = (
    <div
      data-testid='nomi-context-usage-popover'
      className='w-320px max-w-[min(320px,calc(100vw-24px))] box-border px-2px py-1px max-h-[min(70vh,420px)] overflow-y-auto'
    >
      <div className='mb-10px flex items-center justify-between gap-8px'>
        <div className='text-13px font-600 leading-20px text-t-primary'>
          {t('conversation.contextUsage.title', { defaultValue: 'Context Usage' })}
        </div>
        <button
          type='button'
          aria-label={t('conversation.contextUsage.close', { defaultValue: 'Close' })}
          className='inline-flex h-20px w-20px shrink-0 items-center justify-center rd-4px b-none bg-transparent p-0 text-t-tertiary cursor-pointer hover:bg-fill-2 hover:text-t-secondary'
          onClick={() => setVisible(false)}
        >
          <Close theme='outline' size='14' />
        </button>
      </div>

      <div className='mb-8px flex items-baseline justify-between gap-12px'>
        <div className='text-13px font-600 leading-20px text-t-primary tabular-nums'>
          {t('conversation.contextUsage.percentFull', {
            pct: pctFull,
            defaultValue: '{{pct}}% Full',
          })}
        </div>
        <div className='text-12px leading-18px text-t-secondary tabular-nums shrink-0'>
          {t('conversation.contextUsage.tokenSummary', {
            used: usedText,
            max: maxText,
            defaultValue: '~{{used}} / {{max}} Tokens',
          })}
        </div>
      </div>

      <div
        className='mb-12px h-8px overflow-hidden rd-999px bg-fill-3 flex'
        data-testid='nomi-context-usage-bar'
        aria-hidden='true'
      >
        {barSegments.map((segment) => (
          <div
            key={segment.key}
            className='h-full transition-width duration-200'
            style={{ width: `${segment.percentOfWindow}%`, background: segment.color }}
          />
        ))}
      </div>

      <div className='flex flex-col gap-8px'>
        {listSegments.map((segment) => {
          const isSummarized = segment.key === 'summarized_conversation';
          return (
            <div key={segment.key} data-testid={`nomi-context-usage-row-${segment.key}`}>
              <div className='flex items-center justify-between gap-12px text-12px leading-18px'>
                <span className='flex min-w-0 items-center gap-8px text-t-secondary'>
                  <span className='size-8px shrink-0 rd-2px' style={{ background: segment.color }} />
                  {isSummarized && canExpandSummarized ? (
                    <button
                      type='button'
                      className='inline-flex min-w-0 items-center gap-4px b-none bg-transparent p-0 text-12px leading-18px text-t-secondary cursor-pointer hover:text-t-primary'
                      aria-expanded={summarizedOpen}
                      onClick={() => setSummarizedOpen((open) => !open)}
                    >
                      <span className='truncate'>{segmentLabel(segment, t)}</span>
                      {summarizedOpen ? <Up theme='outline' size='12' /> : <Down theme='outline' size='12' />}
                    </button>
                  ) : (
                    <span className='truncate'>{segmentLabel(segment, t)}</span>
                  )}
                </span>
                <span className='shrink-0 tabular-nums text-t-primary'>
                  {formatContextTokenAbbrev(segment.tokens)}
                </span>
              </div>
              {isSummarized && canExpandSummarized && summarizedOpen && summarizedProps && (
                <div
                  data-testid='nomi-context-usage-summarized-props'
                  className='mt-6px ml-16px flex flex-col gap-4px text-11px leading-16px text-t-tertiary'
                >
                  {summarizedProps.trigger != null && (
                    <div>
                      {t('conversation.contextUsage.summarizedProperties.trigger', {
                        value:
                          summarizedProps.trigger === 'manual'
                            ? t('conversation.contextUsage.summarizedProperties.triggerManual', {
                                defaultValue: 'Manual',
                              })
                            : t('conversation.contextUsage.summarizedProperties.triggerAuto', {
                                defaultValue: 'Auto',
                              }),
                        defaultValue: 'Trigger: {{value}}',
                      })}
                    </div>
                  )}
                  {typeof summarizedProps.pre_compact_tokens === 'number' && (
                    <div>
                      {t('conversation.contextUsage.summarizedProperties.preCompactTokens', {
                        value: formatContextTokenAbbrev(summarizedProps.pre_compact_tokens),
                        defaultValue: 'Before compact: {{value}}',
                      })}
                    </div>
                  )}
                  {typeof summarizedProps.messages_summarized === 'number' && (
                    <div>
                      {t('conversation.contextUsage.summarizedProperties.messagesSummarized', {
                        count: summarizedProps.messages_summarized,
                        defaultValue: 'Messages summarized: {{count}}',
                      })}
                    </div>
                  )}
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );

  return (
    <Popover
      trigger='click'
      position='top'
      content={content}
      popupVisible={visible}
      onVisibleChange={(next) => {
        setVisible(next);
        if (!next) setSummarizedOpen(false);
      }}
      unmountOnExit
    >
      <button
        type='button'
        aria-label={ariaLabel}
        aria-expanded={visible}
        data-testid='nomi-context-usage-ring'
        // `ring-2`/`ring-offset-2` look like widths, but the numeric suffix resolves against the
        // theme's numeric colour keys, so both only set a colour custom property — no ring width,
        // no box-shadow chain, no focus ring at all. `ring-2px`/`ring-offset-2px` are the widths.
        className='relative h-22px w-22px shrink-0 rd-999px b-none bg-transparent p-0 cursor-pointer outline-none transition-transform hover:scale-105 active:scale-95 focus-visible:ring-2px focus-visible:ring-[rgb(var(--primary-6))] focus-visible:ring-offset-2px focus-visible:ring-offset-[var(--color-bg-2)]'
        style={{ color: tone }}
      >
        <span aria-hidden='true' className='absolute inset-0 rd-999px' style={{ background: ringFill }} />
        <span
          aria-hidden='true'
          className='absolute inset-3px rd-999px'
          style={{
            background: 'var(--color-bg-2)',
            boxShadow: '0 0 0 1px color-mix(in srgb, var(--color-border-2) 60%, transparent) inset',
          }}
        />
        <span
          aria-hidden='true'
          className='absolute left-1/2 top-1/2 h-6px w-6px -translate-x-1/2 -translate-y-1/2 rd-999px'
          style={{ background: tone }}
        />
      </button>
    </Popover>
  );
}
