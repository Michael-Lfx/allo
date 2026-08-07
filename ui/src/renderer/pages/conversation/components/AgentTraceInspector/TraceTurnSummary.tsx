/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useCallback, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, Message, Tag, Tooltip } from '@arco-design/web-react';
import { Copy } from '@icon-park/react';
import { copyText } from '@renderer/utils/ui/clipboard';
import type { AgentTurnTrace } from './useAgentTraces';
import {
  contextOccupancyPercent,
  formatClock,
  formatElapsed,
  formatJson,
  formatTokenCount,
  outcomeLabel,
  shortId,
} from './format';

export interface TraceTurnSummaryProps {
  trace: AgentTurnTrace;
}

const MetaRow: React.FC<{
  label: string;
  value: string;
  mono?: boolean;
  copyValue?: string;
}> = ({ label, value, mono, copyValue }) => {
  const { t } = useTranslation();
  const onCopy = useCallback(async () => {
    const text = copyValue ?? value;
    if (!text || text === '—') return;
    try {
      await copyText(text);
      Message.success(t('conversation.agentTrace.copied'));
    } catch {
      Message.error(t('conversation.agentTrace.copyFailed'));
    }
  }, [copyValue, t, value]);

  return (
    <div className='flex items-start gap-8px min-w-0'>
      <span className='w-88px shrink-0 text-11px text-[var(--color-text-3)] pt-1px'>{label}</span>
      <span
        className={`flex-1 min-w-0 text-12px text-[var(--color-text-1)] break-all ${
          mono ? 'font-mono' : ''
        }`}
      >
        {value}
      </span>
      {copyValue || (value && value !== '—') ? (
        <Tooltip content={t('conversation.agentTrace.copy')}>
          <Button
            type='text'
            size='mini'
            className='shrink-0 !p-0 !h-18px !w-18px'
            icon={<Copy theme='outline' size='12' strokeWidth={3} />}
            onClick={() => void onCopy()}
            aria-label={t('conversation.agentTrace.copy')}
          />
        </Tooltip>
      ) : null}
    </div>
  );
};

const MetricCell: React.FC<{ label: string; value: React.ReactNode; warn?: boolean }> = ({
  label,
  value,
  warn,
}) => (
  <div className='rounded-4px bg-[var(--color-fill-1)] px-8px py-6px min-w-0'>
    <div className='text-10px text-[var(--color-text-3)] mb-2px'>{label}</div>
    <div
      className={`text-12px font-600 tabular-nums truncate ${
        warn ? 'text-[var(--color-danger-6,#cb272d)]' : 'text-[var(--color-text-1)]'
      }`}
    >
      {value}
    </div>
  </div>
);

const TraceTurnSummary: React.FC<TraceTurnSummaryProps> = ({ trace }) => {
  const { t } = useTranslation();
  const summary = trace.summary;
  const [idsExpanded, setIdsExpanded] = useState(false);

  const outcome = outcomeLabel(summary?.success, summary?.stop_reason);
  const occupancy = contextOccupancyPercent(summary?.context_tokens, summary?.context_window);
  const elapsed =
    summary?.elapsed_ms ??
    (trace.ended_at_ms != null ? trace.ended_at_ms - trace.started_at_ms : null);

  const outcomeTag = useMemo(() => {
    if (outcome === 'fail') {
      return (
        <Tag size='small' color='red'>
          {t('conversation.agentTrace.outcomeFail')}
        </Tag>
      );
    }
    if (outcome === 'cancelled') {
      return (
        <Tag size='small' color='orangered'>
          {t('conversation.agentTrace.outcomeCancelled')}
        </Tag>
      );
    }
    if (outcome === 'ok') {
      return (
        <Tag size='small' color='green'>
          {t('conversation.agentTrace.outcomeOk')}
        </Tag>
      );
    }
    return (
      <Tag size='small' color='gray'>
        {t('conversation.agentTrace.outcomeUnknown')}
      </Tag>
    );
  }, [outcome, t]);

  const copyJson = useCallback(async () => {
    try {
      await copyText(formatJson(trace));
      Message.success(t('conversation.agentTrace.copied'));
    } catch {
      Message.error(t('conversation.agentTrace.copyFailed'));
    }
  }, [t, trace]);

  return (
    <div className='border-b border-solid border-[var(--color-border-2)]'>
      {(summary?.error_code || summary?.error_message || outcome === 'fail') && (
        <div
          className='mx-12px mt-10px mb-2px rounded-4px border border-solid border-[var(--color-danger-3,#f53f3f33)] px-10px py-8px'
          style={{
            background: 'color-mix(in srgb, var(--color-danger-6, #cb272d) 8%, transparent)',
          }}
        >
          <div className='text-12px font-600 text-[var(--color-danger-6,#cb272d)] mb-2px'>
            {t('conversation.agentTrace.errorBanner')}
            {summary?.error_code ? ` · ${summary.error_code}` : ''}
          </div>
          {summary?.error_message ? (
            <div className='text-12px text-[var(--color-text-2)] whitespace-pre-wrap break-all'>
              {summary.error_message}
            </div>
          ) : null}
        </div>
      )}

      <div className='px-12px pt-10px pb-6px flex items-center justify-between gap-8px flex-wrap'>
        <div className='flex items-center gap-8px min-w-0 flex-wrap'>
          {outcomeTag}
          {summary?.stop_reason ? (
            <Tag size='small' color='arcoblue'>
              {summary.stop_reason}
            </Tag>
          ) : null}
          <span className='text-11px text-[var(--color-text-3)]'>{formatClock(trace.started_at_ms)}</span>
        </div>
        <Button type='outline' size='mini' onClick={() => void copyJson()}>
          {t('conversation.agentTrace.copyJson')}
        </Button>
      </div>

      <div className='px-12px pb-8px grid grid-cols-2 sm:grid-cols-3 gap-6px'>
        <MetricCell label={t('conversation.agentTrace.elapsed')} value={formatElapsed(elapsed)} />
        <MetricCell
          label={t('conversation.agentTrace.tokensInOut')}
          value={`${formatTokenCount(summary?.input_tokens)} / ${formatTokenCount(summary?.output_tokens)}`}
        />
        <MetricCell
          label={t('conversation.agentTrace.cache')}
          value={`${formatTokenCount(summary?.cache_read_tokens)} r / ${formatTokenCount(summary?.cache_creation_tokens)} w`}
        />
        <MetricCell
          label={t('conversation.agentTrace.context')}
          value={
            occupancy != null
              ? `${formatTokenCount(summary?.context_tokens)} / ${formatTokenCount(summary?.context_window)} (${occupancy}%)`
              : `${formatTokenCount(summary?.context_tokens)} / ${formatTokenCount(summary?.context_window)}`
          }
          warn={occupancy != null && occupancy >= 85}
        />
        <MetricCell
          label={t('conversation.agentTrace.llmRounds')}
          value={summary?.llm_round_count ?? 0}
        />
        <MetricCell
          label={t('conversation.agentTrace.tools')}
          value={`${summary?.tool_call_count ?? 0}${
            (summary?.tool_error_count ?? 0) > 0
              ? ` (${summary?.tool_error_count} ${t('conversation.agentTrace.errors')})`
              : ''
          }`}
          warn={(summary?.tool_error_count ?? 0) > 0}
        />
      </div>

      {(trace.provider || trace.model) && (
        <div className='px-12px pb-6px text-12px text-[var(--color-text-2)]'>
          <span className='text-[var(--color-text-3)]'>{t('conversation.agentTrace.model')}: </span>
          {[trace.provider, trace.model].filter(Boolean).join(' / ')}
        </div>
      )}

      <div className='px-12px pb-10px'>
        <button
          type='button'
          className='text-11px text-[var(--color-text-3)] bg-transparent border-0 p-0 cursor-pointer hover:text-[var(--color-text-1)]'
          onClick={() => setIdsExpanded((v) => !v)}
        >
          {idsExpanded
            ? t('conversation.agentTrace.hideIds')
            : t('conversation.agentTrace.showIds')}
        </button>
        {idsExpanded ? (
          <div className='mt-6px flex flex-col gap-4px rounded-4px bg-[var(--color-fill-1)] px-8px py-8px'>
            <MetaRow
              label='trace_id'
              value={trace.trace_id}
              mono
              copyValue={trace.trace_id}
            />
            <MetaRow label='msg_id' value={trace.msg_id} mono copyValue={trace.msg_id} />
            <MetaRow
              label='root_turn_id'
              value={trace.root_turn_id}
              mono
              copyValue={trace.root_turn_id}
            />
            <MetaRow
              label={t('conversation.agentTrace.sessionKind')}
              value={trace.session_kind}
            />
            {trace.origin ? <MetaRow label='origin' value={trace.origin} /> : null}
            <MetaRow
              label={t('conversation.agentTrace.startedAt')}
              value={`${formatClock(trace.started_at_ms)} (${trace.started_at_ms})`}
              copyValue={String(trace.started_at_ms)}
            />
            <MetaRow
              label='short'
              value={`trace=${shortId(trace.trace_id)} msg=${shortId(trace.msg_id)}`}
            />
          </div>
        ) : null}
      </div>
    </div>
  );
};

export default TraceTurnSummary;
