/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useCallback, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, Collapse, Input, Message, Tag, Tooltip } from '@arco-design/web-react';
import { Copy } from '@icon-park/react';
import { copyText } from '@renderer/utils/ui/clipboard';
import type { AgentTraceSpan } from './useAgentTraces';
import { spanArtifacts } from './useAgentTraces';
import { ArtifactRowMeta } from './TraceSessionArtifacts';
import { formatElapsed, formatJson, shortId } from './format';

export interface TraceTimelineProps {
  spans: AgentTraceSpan[];
  turnStartedAtMs: number;
}

function spanDurationMs(span: AgentTraceSpan): number | null {
  if (span.ended_at_ms == null) return null;
  return Math.max(0, span.ended_at_ms - span.started_at_ms);
}

function kindColor(kind: string): string {
  switch (kind) {
    case 'llm':
      return 'arcoblue';
    case 'tool':
      return 'cyan';
    case 'thinking':
      return 'purple';
    case 'moa':
      return 'magenta';
    case 'error':
      return 'red';
    case 'system':
      return 'gray';
    case 'compact':
      return 'orangered';
    case 'goal':
      return 'green';
    default:
      return 'gray';
  }
}

function statusColor(status: string): string {
  switch (status) {
    case 'ok':
      return 'green';
    case 'error':
      return 'red';
    case 'cancelled':
      return 'orangered';
    case 'running':
      return 'blue';
    default:
      return 'gray';
  }
}

function attrEntries(attrs: Record<string, unknown> | undefined): [string, unknown][] {
  if (!attrs) return [];
  return Object.entries(attrs).sort(([a], [b]) => a.localeCompare(b));
}

function attrString(
  attrs: Record<string, unknown> | undefined,
  key: string
): string | null {
  const v = attrs?.[key];
  if (v == null) return null;
  if (typeof v === 'string' || typeof v === 'number' || typeof v === 'boolean') {
    return String(v);
  }
  return null;
}

const KIND_FILTERS = ['all', 'llm', 'tool', 'thinking', 'system', 'moa', 'error', 'compact', 'goal'] as const;

const HighlightChips: React.FC<{ span: AgentTraceSpan }> = ({ span }) => {
  const chips: { label: string; value: string }[] = [];
  const toolName = attrString(span.attributes, 'tool_name');
  const callId = attrString(span.attributes, 'call_id');
  const textChars = attrString(span.attributes, 'text_chars');
  const artifactCount = attrString(span.attributes, 'artifact_count');
  if (toolName && toolName !== span.name) chips.push({ label: 'tool', value: toolName });
  if (callId) chips.push({ label: 'call', value: shortId(callId, 10) });
  if (textChars) chips.push({ label: 'chars', value: textChars });
  if (artifactCount && artifactCount !== '0') {
    chips.push({ label: 'arts', value: artifactCount });
  }
  if (chips.length === 0) return null;
  return (
    <span className='hidden sm:inline-flex items-center gap-4px shrink-0 max-w-[40%] overflow-hidden'>
      {chips.map((c) => (
        <span
          key={c.label}
          className='text-10px text-[var(--color-text-3)] truncate'
          title={`${c.label}=${c.value}`}
        >
          {c.label}={c.value}
        </span>
      ))}
    </span>
  );
};

const TraceTimeline: React.FC<TraceTimelineProps> = ({ spans, turnStartedAtMs }) => {
  const { t } = useTranslation();
  const [kindFilter, setKindFilter] = useState<(typeof KIND_FILTERS)[number]>('all');
  const [query, setQuery] = useState('');

  const { maxEnd, ordered } = useMemo(() => {
    if (spans.length === 0) {
      return { maxEnd: turnStartedAtMs + 1, ordered: [] as AgentTraceSpan[] };
    }
    const ordered = [...spans].sort((a, b) => a.started_at_ms - b.started_at_ms);
    const maxEnd = Math.max(
      ...ordered.map((s) => s.ended_at_ms ?? s.started_at_ms),
      turnStartedAtMs + 1
    );
    return { maxEnd, ordered };
  }, [spans, turnStartedAtMs]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return ordered.filter((s) => {
      if (kindFilter !== 'all' && s.kind !== kindFilter) return false;
      if (!q) return true;
      const hay = [
        s.name,
        s.kind,
        s.status,
        s.span_id,
        s.parent_span_id ?? '',
        s.preview ?? '',
        ...Object.entries(s.attributes ?? {}).flatMap(([k, v]) => [
          k,
          typeof v === 'string' ? v : JSON.stringify(v),
        ]),
      ]
        .join('\n')
        .toLowerCase();
      return hay.includes(q);
    });
  }, [kindFilter, ordered, query]);

  const totalMs = Math.max(1, maxEnd - turnStartedAtMs);
  const kindCounts = useMemo(() => {
    const map = new Map<string, number>();
    for (const span of ordered) {
      map.set(span.kind, (map.get(span.kind) ?? 0) + 1);
    }
    return map;
  }, [ordered]);

  const copySpan = useCallback(
    async (span: AgentTraceSpan) => {
      try {
        await copyText(formatJson(span));
        Message.success(t('conversation.agentTrace.copied'));
      } catch {
        Message.error(t('conversation.agentTrace.copyFailed'));
      }
    },
    [t]
  );

  if (ordered.length === 0) {
    return (
      <div className='px-12px py-16px text-12px text-[var(--color-text-3)]'>
        {t('conversation.agentTrace.emptySpans')}
      </div>
    );
  }

  return (
    <div className='flex flex-col gap-8px px-12px py-10px'>
      <div className='flex items-center justify-between gap-8px flex-wrap'>
        <div className='text-11px font-600 text-[var(--color-text-2)]'>
          {t('conversation.agentTrace.spans')} · {ordered.length}
          {filtered.length !== ordered.length ? ` · ${filtered.length}` : ''}
        </div>
        <Input.Search
          allowClear
          size='mini'
          value={query}
          onChange={setQuery}
          placeholder={t('conversation.agentTrace.searchSpans')}
          style={{ width: 200 }}
        />
      </div>

      <div className='flex flex-wrap gap-4px'>
        {KIND_FILTERS.map((kind) => {
          if (kind !== 'all' && !kindCounts.has(kind)) return null;
          const active = kindFilter === kind;
          const count = kind === 'all' ? ordered.length : kindCounts.get(kind) ?? 0;
          return (
            <button
              key={kind}
              type='button'
              className='text-10px px-6px py-2px rounded-2px border border-solid cursor-pointer'
              style={{
                borderColor: active ? 'var(--color-text-2)' : 'var(--color-border-2)',
                background: active
                  ? 'color-mix(in srgb, var(--color-text-2) 10%, transparent)'
                  : 'transparent',
                color: 'var(--color-text-2)',
              }}
              onClick={() => setKindFilter(kind)}
            >
              {kind === 'all' ? t('conversation.agentTrace.filterAll') : kind}
              {` ${count}`}
            </button>
          );
        })}
      </div>

      {/* Overview waterfall */}
      <div className='rounded-4px bg-[var(--color-fill-1)] px-8px py-8px'>
        <div className='text-10px text-[var(--color-text-3)] mb-6px'>
          {t('conversation.agentTrace.waterfall')} · {formatElapsed(totalMs)}
        </div>
        <div className='flex flex-col gap-3px max-h-160px overflow-auto'>
          {filtered.map((span) => {
            const offset = Math.max(0, span.started_at_ms - turnStartedAtMs);
            const duration = spanDurationMs(span) ?? Math.max(4, Math.round(totalMs * 0.02));
            const leftPct = Math.min(100, (offset / totalMs) * 100);
            const widthPct = Math.max(1.5, Math.min(100 - leftPct, (duration / totalMs) * 100));
            const isError = span.status === 'error';
            return (
              <div key={`bar-${span.span_id}`} className='flex items-center gap-6px min-w-0'>
                <span
                  className='w-72px shrink-0 text-10px text-[var(--color-text-3)] truncate'
                  title={span.name}
                >
                  {span.kind === 'tool' ? span.name : span.kind}
                </span>
                <div className='relative flex-1 h-5px rounded-2px bg-[var(--color-fill-2)] overflow-hidden'>
                  <div
                    className='absolute top-0 bottom-0 rounded-2px'
                    style={{
                      left: `${leftPct}%`,
                      width: `${widthPct}%`,
                      background: isError
                        ? 'var(--color-danger-6, #cb272d)'
                        : 'var(--color-text-3)',
                      opacity: 0.9,
                    }}
                  />
                </div>
                <span className='w-48px shrink-0 text-right text-10px text-[var(--color-text-3)] tabular-nums'>
                  {spanDurationMs(span) != null ? formatElapsed(spanDurationMs(span)) : '—'}
                </span>
              </div>
            );
          })}
        </div>
      </div>

      {filtered.length === 0 ? (
        <div className='py-12px text-12px text-[var(--color-text-3)]'>
          {t('conversation.agentTrace.noSpanMatch')}
        </div>
      ) : (
        <Collapse
          bordered={false}
          defaultActiveKey={filtered
            .filter((s) => s.status === 'error')
            .map((s) => s.span_id)
            .slice(0, 5)}
          style={{ background: 'transparent' }}
        >
          {filtered.map((span, index) => {
            const duration = spanDurationMs(span);
            const offset = Math.max(0, span.started_at_ms - turnStartedAtMs);
            const artifacts = spanArtifacts(span.attributes);
            const attrs = attrEntries(span.attributes).filter(([key]) => key !== 'artifacts');
            const header = (
              <div className='flex items-center gap-6px min-w-0 pr-8px w-full'>
                <span className='text-10px text-[var(--color-text-3)] tabular-nums shrink-0 w-18px'>
                  {index + 1}
                </span>
                <Tag size='small' color={kindColor(span.kind)}>
                  {span.kind}
                </Tag>
                <Tag size='small' color={statusColor(span.status)}>
                  {span.status}
                </Tag>
                <span className='text-12px text-[var(--color-text-1)] truncate flex-1 min-w-0'>
                  {span.name}
                </span>
                <HighlightChips span={span} />
                <span className='text-11px text-[var(--color-text-3)] tabular-nums shrink-0'>
                  +{formatElapsed(offset)} · {duration != null ? formatElapsed(duration) : '—'}
                </span>
              </div>
            );

            return (
              <Collapse.Item
                key={span.span_id}
                name={span.span_id}
                header={header}
                style={{
                  border: '1px solid var(--color-border-2)',
                  borderRadius: 4,
                  marginBottom: 6,
                  overflow: 'hidden',
                  background: 'var(--color-bg-1)',
                }}
              >
                <div className='flex flex-col gap-8px text-12px'>
                  <div className='flex justify-end'>
                    <Tooltip content={t('conversation.agentTrace.copyJson')}>
                      <Button
                        type='text'
                        size='mini'
                        className='flowy-icon-text-btn'
                        icon={<Copy theme='outline' size='12' strokeWidth={3} />}
                        onClick={(e) => {
                          e.stopPropagation();
                          void copySpan(span);
                        }}
                      >
                        {t('conversation.agentTrace.copyJson')}
                      </Button>
                    </Tooltip>
                  </div>

                  <div className='grid grid-cols-1 sm:grid-cols-2 gap-4px text-[var(--color-text-2)]'>
                    <div>
                      <span className='text-[var(--color-text-3)]'>span_id · </span>
                      <span className='font-mono break-all'>{span.span_id}</span>
                    </div>
                    {span.parent_span_id ? (
                      <div>
                        <span className='text-[var(--color-text-3)]'>parent · </span>
                        <span className='font-mono break-all'>{span.parent_span_id}</span>
                      </div>
                    ) : null}
                    <div>
                      <span className='text-[var(--color-text-3)]'>
                        {t('conversation.agentTrace.startedAt')} ·{' '}
                      </span>
                      <span className='tabular-nums'>{span.started_at_ms}</span>
                      <span className='text-[var(--color-text-3)]'>
                        {' '}
                        (+{formatElapsed(offset)})
                      </span>
                    </div>
                    <div>
                      <span className='text-[var(--color-text-3)]'>
                        {t('conversation.agentTrace.endedAt')} ·{' '}
                      </span>
                      <span className='tabular-nums'>{span.ended_at_ms ?? '—'}</span>
                      {duration != null ? (
                        <span className='text-[var(--color-text-3)]'>
                          {' '}
                          ({formatElapsed(duration)})
                        </span>
                      ) : null}
                    </div>
                  </div>

                  {span.preview ? (
                    <div>
                      <div className='text-11px text-[var(--color-text-3)] mb-2px'>
                        {t('conversation.agentTrace.preview')}
                      </div>
                      <pre className='m-0 max-h-220px overflow-auto rounded-4px bg-[var(--color-fill-1)] px-8px py-6px text-11px text-[var(--color-text-1)] whitespace-pre-wrap break-all font-mono'>
                        {span.preview}
                      </pre>
                    </div>
                  ) : null}

                  {artifacts.length > 0 ? (
                    <div>
                      <div className='text-11px text-[var(--color-text-3)] mb-2px'>
                        {t('conversation.agentTrace.artifacts')} · {artifacts.length}
                      </div>
                      <div className='rounded-4px bg-[var(--color-fill-1)] px-8px py-6px flex flex-col gap-6px'>
                        {artifacts.map((artifact) => (
                          <ArtifactRowMeta key={artifact.id} artifact={artifact} />
                        ))}
                      </div>
                    </div>
                  ) : null}

                  {attrs.length > 0 ? (
                    <div>
                      <div className='text-11px text-[var(--color-text-3)] mb-2px'>
                        {t('conversation.agentTrace.attributes')}
                      </div>
                      <div className='rounded-4px bg-[var(--color-fill-1)] px-8px py-6px flex flex-col gap-4px'>
                        {attrs.map(([key, value]) => (
                          <div key={key} className='min-w-0'>
                            <div className='text-10px text-[var(--color-text-3)] font-mono'>{key}</div>
                            <pre className='m-0 text-11px text-[var(--color-text-1)] whitespace-pre-wrap break-all font-mono'>
                              {typeof value === 'string' ? value : formatJson(value)}
                            </pre>
                          </div>
                        ))}
                      </div>
                    </div>
                  ) : null}
                </div>
              </Collapse.Item>
            );
          })}
        </Collapse>
      )}
    </div>
  );
};

export default TraceTimeline;
