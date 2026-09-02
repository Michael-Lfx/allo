/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useCallback, useEffect, useId, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, Tooltip } from '@arco-design/web-react';
import {
  Attention,
  CheckOne,
  ExpandLeft,
  ExpandRight,
  MessageOne,
  PlayOne,
  SendOne,
  Tool,
} from '@icon-park/react';
import { formatDurationMs } from './format';
import type { InspectTarget } from './traceSelection';
import {
  type ObservationTimelineEvent,
  type ProjectedGap,
  type ProjectedModelCall,
  type ProjectedTurn,
} from './useAgentTraces';

export const TIMELINE_WAITING_THRESHOLD_MS = 1000;
const TOOL_TERMINAL_EVENTS = new Set([
  'tool/execution_completed',
  'tool/execution_failed',
  'tool/execution_cancelled',
]);

export type TimelineRow = {
  key: string;
  eventSeq: number;
  eventSeqs: number[];
  eventType: string;
  relativeStartMs: number;
  relativeEndMs: number;
  status?: string;
  durationMs?: number;
  toolName?: string;
  target: InspectTarget | null;
  callNumber?: number;
  callKind?: string | null;
  gapReason?: string | null;
  gapFromSeq?: number | null;
  gapToSeq?: number | null;
  responseHasToolUse?: boolean;
};

export type TimelineIconKind =
  | 'start'
  | 'end'
  | 'gap'
  | 'request'
  | 'response-tool'
  | 'response-final'
  | 'tool'
  | 'event';

type Translator = (key: string, options?: Record<string, unknown>) => string;

function statusFromEvent(event: ObservationTimelineEvent): string | undefined {
  if (event.status) return event.status;
  if (event.event_type === 'tool/execution_started') return 'started';
  if (event.event_type === 'tool/execution_completed') return 'completed';
  if (event.event_type === 'tool/execution_failed') return 'failed';
  if (event.event_type === 'tool/execution_cancelled') return 'cancelled';
  return undefined;
}

function targetForEvent(event: ObservationTimelineEvent): InspectTarget | null {
  if (!event.model_call_id) return null;
  if (event.event_type === 'llm/request') {
    return { modelCallId: event.model_call_id, stage: 'request' };
  }
  if (event.event_type === 'llm/response') {
    return { modelCallId: event.model_call_id, stage: 'response' };
  }
  if (event.event_type.startsWith('tool/')) {
    return {
      modelCallId: event.model_call_id,
      stage: 'tool',
      toolCallId: event.tool_call_id ?? undefined,
    };
  }
  return null;
}

function toolMatches(
  started: ObservationTimelineEvent,
  candidate: ObservationTimelineEvent,
): boolean {
  return (
    candidate.event_type.startsWith('tool/execution_') &&
    candidate.model_call_id === started.model_call_id &&
    candidate.tool_call_id != null &&
    candidate.tool_call_id === started.tool_call_id
  );
}

function findToolTerminal(
  events: ObservationTimelineEvent[],
  startIndex: number,
  started: ObservationTimelineEvent,
): ObservationTimelineEvent | undefined {
  if (!started.tool_call_id) return undefined;
  return events
    .slice(startIndex + 1)
    .find(
      (candidate) => TOOL_TERMINAL_EVENTS.has(candidate.event_type) && toolMatches(started, candidate),
    );
}

function callForEvent(
  event: ObservationTimelineEvent,
  callsById: Map<string, ProjectedModelCall>,
): ProjectedModelCall | undefined {
  return event.model_call_id ? callsById.get(event.model_call_id) : undefined;
}

function toolNameForEvent(
  event: ObservationTimelineEvent,
  call: ProjectedModelCall | undefined,
): string | undefined {
  if (event.tool_name?.trim()) return event.tool_name.trim();
  if (!event.tool_call_id || !call) return undefined;
  return call.tools.find((tool) => tool.tool_call_id === event.tool_call_id)?.name?.trim() || undefined;
}

function gapForEvent(
  event: ObservationTimelineEvent,
  gapsBySeq: Map<number, ProjectedGap>,
): ProjectedGap | undefined {
  return event.event_type === 'observation/gap' ? gapsBySeq.get(event.event_seq) : undefined;
}

function makeRow(
  event: ObservationTimelineEvent,
  callsById: Map<string, ProjectedModelCall>,
  callNumbers: Map<string, number>,
  gapsBySeq: Map<number, ProjectedGap>,
): TimelineRow {
  const call = callForEvent(event, callsById);
  const gap = gapForEvent(event, gapsBySeq);
  const durationMs =
    event.duration_ms != null && Number.isFinite(event.duration_ms)
      ? Math.max(0, event.duration_ms)
      : undefined;
  const responseHasToolUse =
    event.event_type === 'llm/response'
      ? Boolean(call?.response_summary?.tool_use_count || call?.tools.length)
      : undefined;

  return {
    key: 'timeline-' + event.event_seq,
    eventSeq: event.event_seq,
    eventSeqs: [event.event_seq],
    eventType: event.event_type,
    relativeStartMs: Math.max(0, event.relative_ms),
    relativeEndMs: Math.max(0, event.relative_ms) + (durationMs ?? 0),
    status: statusFromEvent(event),
    durationMs,
    toolName: toolNameForEvent(event, call),
    target: targetForEvent(event),
    callNumber: event.model_call_id ? callNumbers.get(event.model_call_id) : undefined,
    callKind: event.call_kind,
    gapReason: gap?.reason,
    gapFromSeq: gap?.from_seq,
    gapToSeq: gap?.to_seq,
    responseHasToolUse,
  };
}

/**
 * Projects raw timeline events into compact, selectable UI rows.
 *
 * This deliberately keeps the raw event sequence on every row. A visually
 * merged tool row therefore never changes the export or hides a lifecycle
 * event from a caller that needs the original trace.
 */
export function buildTimelineRows(
  events: ProjectedTurn['timeline'],
  calls: ProjectedModelCall[],
  gaps: ProjectedGap[] = [],
): TimelineRow[] {
  const ordered = [...events].sort((left, right) => left.event_seq - right.event_seq);
  const callsById = new Map(calls.map((call) => [call.model_call_id, call]));
  const callNumbers = new Map(calls.map((call, index) => [call.model_call_id, index + 1]));
  const gapsBySeq = new Map(gaps.map((gap) => [gap.event_seq, gap]));
  const consumed = new Set<number>();
  const rows: TimelineRow[] = [];

  ordered.forEach((event, index) => {
    if (consumed.has(event.event_seq)) return;

    if (event.event_type === 'tool/execution_started') {
      const terminal = findToolTerminal(ordered, index, event);
      if (terminal) {
        const call = callForEvent(event, callsById);
        const gap = gapForEvent(event, gapsBySeq);
        const durationMs =
          terminal.duration_ms != null && Number.isFinite(terminal.duration_ms)
            ? Math.max(0, terminal.duration_ms)
            : Math.max(0, terminal.relative_ms - event.relative_ms);
        rows.push({
          key: 'timeline-' + event.event_seq + '-' + terminal.event_seq,
          eventSeq: event.event_seq,
          eventSeqs: [event.event_seq, terminal.event_seq],
          eventType: 'tool/execution',
          relativeStartMs: Math.max(0, event.relative_ms),
          relativeEndMs: Math.max(0, terminal.relative_ms),
          status: statusFromEvent(terminal) ?? statusFromEvent(event),
          durationMs,
          toolName: toolNameForEvent(event, call) ?? toolNameForEvent(terminal, call),
          target: targetForEvent(event),
          callNumber: event.model_call_id ? callNumbers.get(event.model_call_id) : undefined,
          callKind: event.call_kind ?? terminal.call_kind,
          gapReason: gap?.reason,
          gapFromSeq: gap?.from_seq,
          gapToSeq: gap?.to_seq,
        });
        consumed.add(terminal.event_seq);
        return;
      }
    }

    rows.push(makeRow(event, callsById, callNumbers, gapsBySeq));
  });

  return rows;
}

export function findTimelineEventSeq(
  rows: TimelineRow[],
  target: InspectTarget | null,
): number | null {
  if (!target) return null;
  return (
    rows.find((row) => {
      const rowTarget = row.target;
      return (
        rowTarget?.modelCallId === target.modelCallId &&
        rowTarget.stage === target.stage &&
        rowTarget.toolCallId === target.toolCallId
      );
    })?.eventSeq ?? null
  );
}

export function timelineWaitDurations(rows: TimelineRow[]): number[] {
  return rows
    .slice(1)
    .map((row, index) => Math.max(0, row.relativeStartMs - rows[index].relativeEndMs))
    .filter((duration) => duration >= TIMELINE_WAITING_THRESHOLD_MS);
}

export function timelineRowTitle(t: Translator, row: TimelineRow): string {
  if (row.eventType === 'turn/start') return t('conversation.agentTrace.timelineTurnStart');
  if (row.eventType === 'turn/end') return t('conversation.agentTrace.timelineTurnEnd');
  if (row.eventType === 'observation/gap') return t('conversation.agentTrace.timelineGap');
  if (row.eventType === 'llm/request') return t('conversation.agentTrace.timelineRequest');
  if (row.eventType === 'llm/response') {
    return row.responseHasToolUse
      ? t('conversation.agentTrace.timelineResponseTool')
      : t('conversation.agentTrace.timelineResponseFinal');
  }
  if (row.eventType === 'tool/execution') {
    if (row.status === 'failed') return t('conversation.agentTrace.timelineToolFailed');
    if (row.status === 'cancelled') return t('conversation.agentTrace.timelineToolCancelled');
    if (row.status === 'completed') return t('conversation.agentTrace.timelineToolComplete');
    return t('conversation.agentTrace.timelineToolExecution');
  }
  if (row.eventType === 'tool/execution_started') {
    return t('conversation.agentTrace.timelineToolExecution');
  }
  if (row.eventType === 'tool/execution_completed') {
    return t('conversation.agentTrace.timelineToolComplete');
  }
  if (row.eventType === 'tool/execution_failed') {
    return t('conversation.agentTrace.timelineToolFailed');
  }
  if (row.eventType === 'tool/execution_cancelled') {
    return t('conversation.agentTrace.timelineToolCancelled');
  }
  return row.eventType;
}

export function timelineRowStatusLabel(t: Translator, row: TimelineRow): string {
  if (!row.status) return '';
  if (row.eventType.startsWith('tool/') || row.eventType === 'tool/execution') {
    return t('conversation.agentTrace.tool_' + row.status);
  }
  if (row.status === 'degraded') return t('conversation.agentTrace.integrityDegraded');
  return t('conversation.agentTrace.status_' + row.status);
}

export function timelineRowRelativeTime(row: TimelineRow): string {
  return '+' + (formatDurationMs(row.relativeStartMs) || '0s');
}

export function timelineRowMeta(t: Translator, row: TimelineRow): string {
  return [
    timelineRowRelativeTime(row),
    row.callNumber != null
      ? t('conversation.agentTrace.timelineCall', { n: row.callNumber })
      : null,
    row.toolName || null,
    row.durationMs != null
      ? t('conversation.agentTrace.timelineDuration', {
          duration: formatDurationMs(row.durationMs),
        })
      : null,
    timelineRowStatusLabel(t, row) || null,
    row.callKind && row.callKind !== 'agent_turn'
      ? t('conversation.agentTrace.timelineAuxiliary')
      : null,
  ]
    .filter(Boolean)
    .join(' · ');
}

function eventAriaLabel(t: Translator, row: TimelineRow): string {
  const meta = timelineRowMeta(t, row);
  return [timelineRowTitle(t, row), meta, '#' + row.eventSeq].filter(Boolean).join(' · ');
}

export function timelineRowIconKind(row: TimelineRow): TimelineIconKind {
  if (row.eventType === 'turn/start') return 'start';
  if (row.eventType === 'turn/end') return 'end';
  if (row.eventType === 'observation/gap') return 'gap';
  if (row.eventType === 'llm/request') return 'request';
  if (row.eventType === 'tool/execution' || row.eventType.startsWith('tool/')) return 'tool';
  if (row.eventType === 'llm/response') {
    return row.responseHasToolUse ? 'response-tool' : 'response-final';
  }
  return 'event';
}

const TimelineEventIcon: React.FC<{ row: TimelineRow }> = ({ row }) => {
  const props = { theme: 'outline' as const, size: '14', strokeWidth: 3 };
  switch (timelineRowIconKind(row)) {
    case 'start':
      return <PlayOne {...props} />;
    case 'end':
      return <CheckOne {...props} />;
    case 'gap':
      return <Attention {...props} />;
    case 'request':
      return <SendOne {...props} />;
    case 'response-tool':
    case 'tool':
      return <Tool {...props} />;
    case 'response-final':
      return <CheckOne {...props} />;
    default:
      return <MessageOne {...props} />;
  }
};

export interface ObservationTimelinePanelProps {
  rootTurnId: string;
  events: ProjectedTurn['timeline'];
  calls: ProjectedModelCall[];
  gaps: ProjectedGap[];
  selectedEventSeq: number | null;
  collapsed: boolean;
  onToggleCollapsed: () => void;
  onSelect: (row: TimelineRow) => void;
}

export const ObservationTimelinePanel: React.FC<ObservationTimelinePanelProps> = ({
  rootTurnId,
  events,
  calls,
  gaps,
  selectedEventSeq,
  collapsed,
  onToggleCollapsed,
  onSelect,
}) => {
  const { t } = useTranslation();
  const generatedId = useId().replace(/:/g, '');
  const eventsId = 'session-logs-timeline-events-' + generatedId;
  const scrollRef = useRef<HTMLDivElement>(null);
  const latestSeqRef = useRef<number | null>(null);
  const atBottomRef = useRef(true);
  const timelineToggleTooltipSuppressedRef = useRef(false);
  const [hasNewEvents, setHasNewEvents] = useState(false);
  const [timelineToggleTooltipVisible, setTimelineToggleTooltipVisible] = useState(false);
  const rows = useMemo(() => buildTimelineRows(events, calls, gaps), [events, calls, gaps]);
  const maxEventSeq = events.reduce((max, event) => Math.max(max, event.event_seq), 0);
  const selectedRow = rows.find((row) => row.eventSeqs.includes(selectedEventSeq ?? -1));

  const updateBottomState = useCallback(() => {
    const element = scrollRef.current;
    if (!element) return;
    atBottomRef.current =
      element.scrollHeight - element.clientHeight - element.scrollTop <= 12;
    if (atBottomRef.current) setHasNewEvents(false);
  }, []);

  useEffect(() => {
    latestSeqRef.current = maxEventSeq;
    atBottomRef.current = true;
    setHasNewEvents(false);
  }, [rootTurnId]);

  useEffect(() => {
    setTimelineToggleTooltipVisible(false);
  }, [collapsed]);

  useEffect(() => {
    const previousSeq = latestSeqRef.current;
    latestSeqRef.current = maxEventSeq;
    if (previousSeq == null || maxEventSeq <= previousSeq) return;
    const element = scrollRef.current;
    if (atBottomRef.current && element) {
      const frame = window.requestAnimationFrame(() => {
        element.scrollTop = element.scrollHeight;
      });
      return () => window.cancelAnimationFrame(frame);
    }
    setHasNewEvents(true);
  }, [maxEventSeq]);

  useEffect(() => {
    const element = scrollRef.current;
    if (!element) return;
    updateBottomState();
    element.addEventListener('scroll', updateBottomState, { passive: true });
    return () => element.removeEventListener('scroll', updateBottomState);
  }, [updateBottomState]);

  const showLatest = () => {
    const element = scrollRef.current;
    if (element) element.scrollTop = element.scrollHeight;
    atBottomRef.current = true;
    setHasNewEvents(false);
  };

  return (
    <section
      className={['session-logs-timeline-column', collapsed ? 'is-collapsed' : '']
        .filter(Boolean)
        .join(' ')}
      aria-labelledby={eventsId + '-title'}
    >
      <div className='session-logs-timeline__header'>
        <div className='session-logs-timeline__heading'>
          <div id={eventsId + '-title'} className='session-logs-timeline__title'>
            {t('conversation.agentTrace.timeline')}
          </div>
          <span
            className='session-logs-timeline__count'
            aria-label={t('conversation.agentTrace.timelineEventCount', { count: rows.length })}
          >
            <span className='session-logs-timeline__count-full'>
              {t('conversation.agentTrace.timelineEventCount', { count: rows.length })}
            </span>
            <span className='session-logs-timeline__count-compact' aria-hidden='true'>
              {rows.length}
            </span>
          </span>
          {selectedRow ? (
            <span
              className='session-logs-timeline__current'
              title={timelineRowTitle(t, selectedRow)}
            >
              {timelineRowTitle(t, selectedRow)}
            </span>
          ) : null}
        </div>
        <span
          className='session-logs-timeline__toggle-wrap'
          onMouseLeave={() => {
            timelineToggleTooltipSuppressedRef.current = false;
            setTimelineToggleTooltipVisible(false);
          }}
        >
          <Tooltip
            content={
              collapsed
                ? t('conversation.agentTrace.expandTimeline')
                : t('conversation.agentTrace.collapseTimeline')
            }
            popupVisible={timelineToggleTooltipVisible}
            onVisibleChange={(visible) => {
              if (visible && timelineToggleTooltipSuppressedRef.current) return;
              setTimelineToggleTooltipVisible(visible);
            }}
          >
            <Button
              type='text'
              size='mini'
              className='session-logs-json-tree__icon-btn session-logs-timeline__toggle'
              icon={
                collapsed ? (
                  <ExpandRight theme='outline' size='13' strokeWidth={3} />
                ) : (
                  <ExpandLeft theme='outline' size='13' strokeWidth={3} />
                )
              }
              aria-expanded={!collapsed}
              aria-controls={eventsId}
              aria-label={
                collapsed
                  ? t('conversation.agentTrace.expandTimeline')
                  : t('conversation.agentTrace.collapseTimeline')
              }
              onClick={() => {
                timelineToggleTooltipSuppressedRef.current = true;
                setTimelineToggleTooltipVisible(false);
                onToggleCollapsed();
              }}
              onBlur={() => {
                timelineToggleTooltipSuppressedRef.current = false;
                setTimelineToggleTooltipVisible(false);
              }}
            />
          </Tooltip>
        </span>
      </div>
      <div ref={scrollRef} id={eventsId} className='session-logs-timeline-wrap'>
        {hasNewEvents ? (
          <button
            type='button'
            className='session-logs-timeline-new'
            aria-live='polite'
            onClick={showLatest}
          >
            {t('conversation.agentTrace.timelineNewEvents')}
          </button>
        ) : null}
        {rows.length === 0 ? (
          <div className='session-logs-timeline-empty'>
            {t('conversation.agentTrace.timelineNoEvents')}
          </div>
        ) : (
          <ol className='session-logs-timeline__list'>
            {rows.map((row, index) => {
              const previous = rows[index - 1];
              const waitingMs = previous
                ? Math.max(0, row.relativeStartMs - previous.relativeEndMs)
                : 0;
              const selected = row.eventSeqs.includes(selectedEventSeq ?? -1);
              const title = timelineRowTitle(t, row);
              const meta = timelineRowMeta(t, row);
              return (
                <React.Fragment key={row.key}>
                  {waitingMs >= TIMELINE_WAITING_THRESHOLD_MS ? (
                    <li className='session-logs-timeline__gap' aria-hidden='true'>
                      {t('conversation.agentTrace.timelineWaiting', {
                        duration: formatDurationMs(waitingMs),
                      })}
                    </li>
                  ) : null}
                  <li
                    className={[
                      'session-logs-timeline__item',
                      selected ? 'is-selected' : '',
                      row.status === 'degraded' ? 'is-degraded' : '',
                      row.target ? 'is-call-event' : 'is-meta-event',
                    ]
                      .filter(Boolean)
                      .join(' ')}
                    data-event-seq={row.eventSeq}
                    data-event-seqs={row.eventSeqs.join(',')}
                  >
                    <button
                      type='button'
                      className='session-logs-timeline__event'
                      aria-pressed={selected}
                      aria-current={selected ? 'step' : undefined}
                      aria-label={eventAriaLabel(t, row)}
                      title={eventAriaLabel(t, row)}
                      onClick={() => onSelect(row)}
                    >
                      <span className='session-logs-timeline__rail' aria-hidden='true'>
                        <span className='session-logs-timeline__dot'>
                          <TimelineEventIcon row={row} />
                        </span>
                      </span>
                      <span className='session-logs-timeline__content'>
                        {collapsed ? (
                          <span className='session-logs-timeline__collapsed-time'>
                            {timelineRowRelativeTime(row)}
                          </span>
                        ) : (
                          <>
                            <span className='session-logs-timeline__head'>
                              <span className='session-logs-timeline__label'>{title}</span>
                              <span className='session-logs-timeline__seq'>#{row.eventSeq}</span>
                            </span>
                            <span className='session-logs-timeline__meta'>{meta}</span>
                            {row.gapReason ? (
                              <span className='session-logs-timeline__note'>{row.gapReason}</span>
                            ) : null}
                          </>
                        )}
                      </span>
                    </button>
                  </li>
                </React.Fragment>
              );
            })}
          </ol>
        )}
      </div>
    </section>
  );
};

export default ObservationTimelinePanel;
