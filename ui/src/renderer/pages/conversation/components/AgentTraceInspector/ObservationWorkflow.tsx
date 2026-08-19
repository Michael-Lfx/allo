/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useCallback, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { useVirtualizer } from '@tanstack/react-virtual';
import { Button, Collapse, Message, Spin, Tag, Tooltip } from '@arco-design/web-react';
import { Copy } from '@icon-park/react';
import { copyText } from '@renderer/utils/ui/clipboard';
import { formatJson, shortId } from './format';
import {
  asRecord,
  canonicalRequestFromPayload,
  toolStatus,
  type ProjectedGap,
  type ProjectedModelCall,
  type ProjectedTokenUsage,
  type ProjectedToolExecution,
  type ProjectedTurn,
} from './useAgentTraces';

export interface ObservationWorkflowProps {
  turn: ProjectedTurn;
  expandedCallId: string | null;
  callDetail: ProjectedModelCall | null;
  callLoading: boolean;
  callErrorKey: 'loadFailed' | 'developerModeRequired' | 'retentionRemoved' | null;
  onToggleCall: (modelCallId: string) => void;
}

const MetaRow: React.FC<{
  label: string;
  value: string;
  mono?: boolean;
}> = ({ label, value, mono }) => {
  const { t } = useTranslation();
  const onCopy = useCallback(async () => {
    if (!value || value === '—') return;
    try {
      await copyText(value);
      Message.success(t('conversation.agentTrace.copied'));
    } catch {
      Message.error(t('conversation.agentTrace.copyFailed'));
    }
  }, [t, value]);

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
      {value && value !== '—' ? (
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

const JsonBlock: React.FC<{ label: string; value: unknown }> = ({ label, value }) => {
  const { t } = useTranslation();
  const onCopy = useCallback(async () => {
    try {
      await copyText(formatJson(value));
      Message.success(t('conversation.agentTrace.copied'));
    } catch {
      Message.error(t('conversation.agentTrace.copyFailed'));
    }
  }, [t, value]);

  return (
    <div className='min-w-280px flex-1'>
      <div className='flex items-center justify-between gap-8px mb-2px'>
        <div className='text-11px font-600 text-[var(--color-text-2)]'>{label}</div>
        <Tooltip content={t('conversation.agentTrace.copyJson')}>
          <Button
            type='text'
            size='mini'
            className='flowy-icon-text-btn'
            icon={<Copy theme='outline' size='12' strokeWidth={3} />}
            onClick={() => void onCopy()}
          >
            {t('conversation.agentTrace.copyJson')}
          </Button>
        </Tooltip>
      </div>
      <pre className='m-0 max-h-280px overflow-auto rounded-4px bg-[var(--color-fill-1)] px-8px py-6px text-11px text-[var(--color-text-1)] whitespace-pre-wrap break-all font-mono'>
        {formatJson(value)}
      </pre>
    </div>
  );
};

function toolStatusColor(status: ReturnType<typeof toolStatus>): string {
  switch (status) {
    case 'failed':
      return 'red';
    case 'cancelled':
      return 'orangered';
    case 'completed':
      return 'green';
    default:
      return 'arcoblue';
  }
}

function collectOmitted(payload: unknown): Array<Record<string, unknown>> {
  const record = asRecord(payload);
  if (!record) return [];
  const notes: Array<Record<string, unknown>> = [];
  const request = asRecord(record.request) ?? record;
  if (Array.isArray(request.omitted)) {
    for (const item of request.omitted) {
      const note = asRecord(item);
      if (note) notes.push(note);
    }
  }
  if (typeof request.omitted_reason === 'string') {
    notes.push({
      field: 'payload',
      reason: request.omitted_reason,
      original_bytes: request.original_bytes,
      captured_bytes: request.captured_bytes,
    });
  }
  if (typeof record.omitted_reason === 'string') {
    notes.push({
      field: 'event',
      reason: record.omitted_reason,
      original_bytes: record.original_bytes,
      captured_bytes: record.captured_bytes,
    });
  }
  return notes;
}

const OmittedNotes: React.FC<{ payload: unknown }> = ({ payload }) => {
  const { t } = useTranslation();
  const notes = collectOmitted(payload);
  if (notes.length === 0) return null;
  return (
    <div className='flex flex-col gap-4px'>
      {notes.map((note, index) => (
        <div
          key={`${String(note.field ?? 'field')}-${index}`}
          className='rounded-4px border border-dashed border-[var(--color-border-3)] px-8px py-6px text-11px text-[var(--color-text-2)]'
        >
          <div className='font-600'>
            {t('conversation.agentTrace.omittedField')}
            {note.field != null ? ` · ${String(note.field)}` : ''}
          </div>
          {note.reason != null ? (
            <div>
              {t('conversation.agentTrace.omittedReason')}: {String(note.reason)}
            </div>
          ) : null}
          {note.original_bytes != null ? (
            <div>
              {t('conversation.agentTrace.originalBytes')}: {String(note.original_bytes)}
            </div>
          ) : null}
          {note.captured_bytes != null ? (
            <div>
              {t('conversation.agentTrace.capturedBytes')}: {String(note.captured_bytes)}
            </div>
          ) : null}
        </div>
      ))}
    </div>
  );
};

const TokenChips: React.FC<{ usage?: ProjectedTokenUsage | null }> = ({ usage }) => {
  const { t } = useTranslation();
  if (!usage) return null;
  const chips: Array<{ key: string; label: string; value: number }> = [];
  if (usage.input_tokens != null) {
    chips.push({ key: 'in', label: t('conversation.agentTrace.tokenInput'), value: usage.input_tokens });
  }
  if (usage.cache_read_tokens != null) {
    chips.push({
      key: 'cr',
      label: t('conversation.agentTrace.tokenCacheRead'),
      value: usage.cache_read_tokens,
    });
  }
  if (usage.cache_creation_tokens != null) {
    chips.push({
      key: 'cw',
      label: t('conversation.agentTrace.tokenCacheWrite'),
      value: usage.cache_creation_tokens,
    });
  }
  if (usage.output_tokens != null) {
    chips.push({
      key: 'out',
      label: t('conversation.agentTrace.tokenOutput'),
      value: usage.output_tokens,
    });
  }
  if (chips.length === 0) return null;
  return (
    <div className='flex items-center gap-6px flex-wrap'>
      {chips.map((chip) => (
        <span
          key={chip.key}
          className='text-10px font-mono text-[var(--color-text-3)] tabular-nums'
        >
          {chip.label}={chip.value}
        </span>
      ))}
    </div>
  );
};

const ToolBlock: React.FC<{ tool: ProjectedToolExecution }> = ({ tool }) => {
  const { t } = useTranslation();
  const status = toolStatus(tool);
  const payload = tool.cancelled ?? tool.failed ?? tool.completed ?? tool.started ?? {};
  return (
    <div className='rounded-4px border border-solid border-[var(--color-border-2)] px-8px py-8px min-w-220px'>
      <div className='flex items-center gap-6px min-w-0 mb-6px'>
        <Tag size='small' color={toolStatusColor(status)}>
          {t(`conversation.agentTrace.tool_${status}`)}
        </Tag>
        <span className='text-12px text-[var(--color-text-1)] truncate'>
          {tool.name ?? t('conversation.agentTrace.tools')}
        </span>
        <span className='text-11px font-mono text-[var(--color-text-3)] truncate' title={tool.tool_call_id}>
          {shortId(tool.tool_call_id)}
        </span>
      </div>
      <pre className='m-0 max-h-200px overflow-auto text-11px text-[var(--color-text-1)] whitespace-pre-wrap break-all font-mono'>
        {formatJson(payload)}
      </pre>
    </div>
  );
};

const GapRow: React.FC<{ gap: ProjectedGap }> = ({ gap }) => {
  const { t } = useTranslation();
  return (
    <div className='rounded-4px border border-dashed border-[var(--color-warning-6,#ff7d00)] px-8px py-6px text-12px text-[var(--color-text-2)]'>
      <div className='font-600 text-[var(--color-warning-6,#ff7d00)]'>
        {t('conversation.agentTrace.gap')}
        {gap.reason ? ` · ${gap.reason}` : ''}
      </div>
      <div className='text-11px text-[var(--color-text-3)] font-mono mt-2px'>
        seq={gap.event_seq}
        {gap.from_seq != null ? ` · from=${gap.from_seq}` : ''}
        {gap.to_seq != null ? ` · to=${gap.to_seq}` : ''}
      </div>
    </div>
  );
};

function statusColor(status: ProjectedModelCall['status'] | ProjectedTurn['status']): string {
  switch (status) {
    case 'failed':
      return 'red';
    case 'cancelled':
    case 'interrupted':
      return 'orangered';
    case 'truncated':
      return 'orange';
    case 'completed':
      return 'green';
    case 'running':
      return 'arcoblue';
    default:
      return 'gray';
  }
}

const ModelCallCard: React.FC<{
  header: ProjectedModelCall;
  index: number;
  expanded: boolean;
  detail: ProjectedModelCall | null;
  loading: boolean;
  errorKey: ObservationWorkflowProps['callErrorKey'];
  onToggle: () => void;
}> = ({ header, index, expanded, detail, loading, errorKey, onToggle }) => {
  const { t } = useTranslation();
  const call = detail ?? header;
  const requestBody = canonicalRequestFromPayload(call.request);
  const responseRecord = asRecord(call.response);
  const status = header.status ?? (header.interrupted ? 'interrupted' : undefined);

  return (
    <div className='flex flex-col gap-8px px-12px py-10px border-b border-solid border-[var(--color-border-2)]'>
      <button
        type='button'
        className='flex items-center gap-6px flex-wrap text-left border-0 bg-transparent p-0 cursor-pointer min-w-0'
        onClick={onToggle}
        aria-expanded={expanded}
        aria-label={
          expanded
            ? t('conversation.agentTrace.collapseCall')
            : t('conversation.agentTrace.expandCall')
        }
      >
        <span className='text-10px text-[var(--color-text-3)] tabular-nums'>{index + 1}</span>
        {status ? (
          <Tag size='small' color={statusColor(status)}>
            {t(`conversation.agentTrace.status_${status}`)}
          </Tag>
        ) : null}
        <Tag size='small' color='arcoblue'>
          {header.call_kind ?? t('conversation.agentTrace.callKind')}
        </Tag>
        {header.observation_scope ? (
          <Tag size='small' color='gray'>
            {header.observation_scope}
          </Tag>
        ) : null}
        {header.interrupted ? (
          <Tag size='small' color='orangered'>
            {t('conversation.agentTrace.interrupted')}
          </Tag>
        ) : null}
        <span className='text-11px font-mono text-[var(--color-text-3)]' title={header.model_call_id}>
          {shortId(header.model_call_id)}
        </span>
        <TokenChips usage={header.usage} />
      </button>

      {expanded ? (
        loading && !detail ? (
          <div className='flex justify-center py-16px'>
            <Spin />
          </div>
        ) : errorKey === 'retentionRemoved' ? (
          <div className='text-12px text-[var(--color-text-2)]'>
            {t('conversation.agentTrace.retentionRemoved')}
          </div>
        ) : errorKey ? (
          <div className='text-12px text-[var(--color-text-2)]'>
            {t(`conversation.agentTrace.${errorKey}`)}
          </div>
        ) : (
          <div className='overflow-x-auto'>
            <div className='flex gap-12px items-start min-w-min'>
              {call.request != null ? (
                <JsonBlock label={t('conversation.agentTrace.request')} value={requestBody} />
              ) : (
                <div className='text-12px text-[var(--color-text-3)] min-w-220px'>
                  {t('conversation.agentTrace.request')} · {t('conversation.agentTrace.previewMissing')}
                </div>
              )}
              {call.response != null ? (
                <JsonBlock
                  label={t('conversation.agentTrace.response')}
                  value={
                    responseRecord
                      ? {
                          text: responseRecord.text,
                          thinking: responseRecord.thinking,
                          tool_use: responseRecord.tool_use,
                          stop_reason: responseRecord.stop_reason,
                          usage: responseRecord.usage,
                          error: responseRecord.error,
                          elapsed_ms: responseRecord.elapsed_ms,
                          ttft_ms: responseRecord.ttft_ms,
                        }
                      : call.response
                  }
                />
              ) : (
                <div className='rounded-4px bg-[var(--color-fill-1)] px-8px py-8px text-12px text-[var(--color-text-2)] min-w-220px'>
                  {t('conversation.agentTrace.noResponse')}
                </div>
              )}
              {call.tools.length > 0 ? (
                <div className='flex flex-col gap-6px min-w-220px'>
                  <div className='text-11px font-600 text-[var(--color-text-2)]'>
                    {t('conversation.agentTrace.tools')} · {call.tools.length}
                  </div>
                  {call.tools.map((tool) => (
                    <ToolBlock key={tool.tool_call_id || formatJson(tool)} tool={tool} />
                  ))}
                </div>
              ) : null}
            </div>
            <div className='mt-8px flex flex-col gap-6px'>
              <OmittedNotes payload={call.request} />
              <OmittedNotes payload={call.response} />
            </div>
          </div>
        )
      ) : (
        <div className='text-11px text-[var(--color-text-4)]'>
          {t('conversation.agentTrace.loadCall')}
        </div>
      )}
    </div>
  );
};

const ObservationWorkflow: React.FC<ObservationWorkflowProps> = ({
  turn,
  expandedCallId,
  callDetail,
  callLoading,
  callErrorKey,
  onToggleCall,
}) => {
  const { t } = useTranslation();
  const parentRef = useRef<HTMLDivElement>(null);
  const virtualizer = useVirtualizer({
    count: turn.model_calls.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 88,
    overscan: 4,
    getItemKey: (index) => turn.model_calls[index]?.model_call_id || index,
  });

  const copyJson = useCallback(async () => {
    try {
      await copyText(formatJson(turn));
      Message.success(t('conversation.agentTrace.copied'));
    } catch {
      Message.error(t('conversation.agentTrace.copyFailed'));
    }
  }, [t, turn]);

  const degraded = turn.integrity === 'degraded';

  return (
    <div className='flex flex-col min-h-0 h-full'>
      {(degraded || turn.interrupted || turn.gap_count > 0) && (
        <div
          className='mx-12px mt-10px mb-2px rounded-4px border border-solid px-10px py-8px shrink-0'
          style={{
            borderColor: 'var(--color-warning-6,#ff7d00)',
            background: 'color-mix(in srgb, var(--color-warning-6, #ff7d00) 8%, transparent)',
          }}
        >
          <div className='text-12px font-600 text-[var(--color-warning-6,#ff7d00)] mb-2px'>
            {t('conversation.agentTrace.integrityDegraded')}
            {turn.interrupted ? ` · ${t('conversation.agentTrace.interrupted')}` : ''}
            {turn.gap_count > 0
              ? ` · ${t('conversation.agentTrace.gapCount', { count: turn.gap_count })}`
              : ''}
          </div>
          <div className='text-12px text-[var(--color-text-2)]'>
            {t('conversation.agentTrace.gapBanner')}
          </div>
        </div>
      )}

      <div className='px-12px pt-10px pb-6px flex items-center justify-between gap-8px flex-wrap shrink-0'>
        <div className='flex items-center gap-8px min-w-0 flex-wrap'>
          <Tag size='small' color={degraded ? 'orangered' : 'green'}>
            {degraded
              ? t('conversation.agentTrace.integrityDegraded')
              : t('conversation.agentTrace.integrityComplete')}
          </Tag>
          {turn.status ? (
            <Tag size='small' color={statusColor(turn.status)}>
              {t(`conversation.agentTrace.status_${turn.status}`)}
            </Tag>
          ) : null}
          {turn.session_kind ? <Tag size='small'>{turn.session_kind}</Tag> : null}
        </div>
        <Button type='outline' size='mini' onClick={() => void copyJson()}>
          {t('conversation.agentTrace.copyJson')}
        </Button>
      </div>

      <div className='px-12px pb-10px shrink-0'>
        <Collapse bordered={false} defaultActiveKey={['ids']} style={{ background: 'transparent' }}>
          <Collapse.Item
            name='ids'
            header={t('conversation.agentTrace.showIds')}
            style={{
              border: '1px solid var(--color-border-2)',
              borderRadius: 4,
              overflow: 'hidden',
              background: 'var(--color-bg-1)',
            }}
          >
            <div className='flex flex-col gap-4px'>
              <MetaRow label='root_turn_id' value={turn.root_turn_id} mono />
              <MetaRow label='msg_id' value={turn.msg_id ?? '—'} mono />
              <MetaRow label='execution_id' value={turn.execution_id ?? '—'} mono />
              <MetaRow
                label='execution_attempt_id'
                value={turn.execution_attempt_id ?? '—'}
                mono
              />
              <MetaRow
                label={t('conversation.agentTrace.sessionKind')}
                value={turn.session_kind ?? '—'}
              />
            </div>
          </Collapse.Item>
        </Collapse>
      </div>

      {turn.gaps.length > 0 ? (
        <div className='px-12px pb-8px flex flex-col gap-6px shrink-0'>
          {turn.gaps.map((gap) => (
            <GapRow key={`gap-${gap.event_seq}`} gap={gap} />
          ))}
        </div>
      ) : null}

      {turn.model_calls.length === 0 ? (
        <div className='px-12px py-16px text-12px text-[var(--color-text-3)]'>
          {t('conversation.agentTrace.emptyWorkflow')}
        </div>
      ) : (
        <div ref={parentRef} className='flex-1 min-h-0 overflow-auto'>
          <div
            style={{
              height: `${virtualizer.getTotalSize()}px`,
              width: '100%',
              position: 'relative',
            }}
          >
            {virtualizer.getVirtualItems().map((item) => {
              const call = turn.model_calls[item.index];
              if (!call) return null;
              return (
                <div
                  key={item.key}
                  data-index={item.index}
                  ref={virtualizer.measureElement}
                  style={{
                    position: 'absolute',
                    top: 0,
                    left: 0,
                    width: '100%',
                    transform: `translateY(${item.start}px)`,
                  }}
                >
                  <ModelCallCard
                    header={call}
                    index={item.index}
                    expanded={expandedCallId === call.model_call_id}
                    detail={expandedCallId === call.model_call_id ? callDetail : null}
                    loading={expandedCallId === call.model_call_id && callLoading}
                    errorKey={expandedCallId === call.model_call_id ? callErrorKey : null}
                    onToggle={() => onToggleCall(call.model_call_id)}
                  />
                </div>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
};

export default ObservationWorkflow;
