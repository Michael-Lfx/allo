/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import classNames from 'classnames';
import { useTranslation } from 'react-i18next';
import { Button, Empty, Popover, Spin, Tooltip } from '@arco-design/web-react';
import { Bug, Info, Refresh, SortAmountDown, SortAmountUp } from '@icon-park/react';
import { isBackendHttpError } from '@/common/adapter/httpBridge';
import type { ConversationId } from '@/common/types/ids';
import { useConfig } from '@/renderer/hooks/config/useConfig';
import {
  capabilityHeaderButtonClass,
  capabilityHeaderButtonStyle,
} from '../CapabilityHeaderButton';
import { CallDetailLru, callCacheKey } from './callDetailCache';
import ObservationWorkflow, { type InspectTarget } from './ObservationWorkflow';
import { formatClock, formatDurationMs, assignTurnRounds, turnToolCount } from './format';
import { sessionLogsOverlayOpen, shouldCloseWorkspaceOnEscape } from './scanCopy';
import './session-logs.css';
import {
  getSessionObservationCall,
  getSessionObservationTurn,
  isObservationRetentionError,
  listSessionObservations,
  type ObservationSummary,
  type ProjectedModelCall,
  type ProjectedTurn,
  type RecorderHealth,
  type SessionObservationList,
} from './useAgentTraces';

export type ConversationColumnView = 'dialogue' | 'logs';

export interface SessionLogsRootProps {
  conversationId: ConversationId;
  view: ConversationColumnView;
  onViewChange: (view: ConversationColumnView) => void;
  children: React.ReactNode;
}

const OBSERVE_ACCENT = 'rgb(var(--primary-6))';
const POLL_START_MS = 1500;
const POLL_MAX_MS = 10000;

function nextPollDelay(current: number): number {
  if (current < 3000) return 3000;
  if (current < 5000) return 5000;
  return POLL_MAX_MS;
}

function healthIsFault(health: RecorderHealth | null): boolean {
  return (
    health != null &&
    (health.status === 'storage_error' || health.status === 'writer_disconnected')
  );
}

function shouldPollTurns(turns: ProjectedTurn[], health: RecorderHealth | null): boolean {
  if (healthIsFault(health)) return false;
  return turns.some((turn) => turn.has_turn_start === true && turn.has_turn_end !== true);
}

function isAbortError(error: unknown): boolean {
  return (
    (error instanceof DOMException && error.name === 'AbortError') ||
    (error instanceof Error && error.name === 'AbortError')
  );
}

const MetricStat: React.FC<{
  value: string | number;
  label: string;
}> = ({ value, label }) => (
  <div className='session-logs-metrics__item'>
    <span className='session-logs-metrics__value'>{value}</span>
    <span className='session-logs-metrics__label'>{label}</span>
  </div>
);

function newestTurnId(turns: ProjectedTurn[]): string | null {
  if (turns.length === 0) return null;
  let best = turns[turns.length - 1];
  for (const turn of turns) {
    if (
      turn.started_at_ms != null &&
      (best.started_at_ms == null || turn.started_at_ms >= best.started_at_ms)
    ) {
      best = turn;
    }
  }
  return best.root_turn_id;
}

function resolveSelectedId(ordered: ProjectedTurn[], current: string | null): string | null {
  if (ordered.length === 0) return null;
  if (current && ordered.some((row) => row.root_turn_id === current)) return current;
  return newestTurnId(ordered);
}

type SessionLogsContextValue = {
  conversationId: ConversationId;
  view: ConversationColumnView;
  onViewChange: (view: ConversationColumnView) => void;
  developerMode: boolean;
  loading: boolean;
  errorKey: 'loadFailed' | 'developerModeRequired' | null;
  entries: ProjectedTurn[];
  summary: ObservationSummary | null;
  health: RecorderHealth | null;
  selectedId: string | null;
  setSelectedId: (id: string) => void;
  detail: ProjectedTurn | null;
  detailLoading: boolean;
  detailErrorKey: 'loadFailed' | 'developerModeRequired' | null;
  inspectTarget: InspectTarget | null;
  setInspectTarget: React.Dispatch<React.SetStateAction<InspectTarget | null>>;
  callDetail: ProjectedModelCall | null;
  callLoading: boolean;
  callErrorKey: 'loadFailed' | 'developerModeRequired' | 'retentionRemoved' | null;
  refreshWorkspace: (options?: { signal?: AbortSignal; showListLoading?: boolean }) => Promise<{
    seqChanged: boolean;
  } | void>;
};

const SessionLogsContext = createContext<SessionLogsContextValue | null>(null);

function useSessionLogs(): SessionLogsContextValue {
  const value = useContext(SessionLogsContext);
  if (!value) {
    throw new Error('Session logs controls require SessionLogsRoot');
  }
  return value;
}

/**
 * Developer Mode–gated session logs: navigator + summary + lazy Call GET.
 * Renders persisted observation projections only — never chat bubbles.
 */
export const SessionLogsRoot: React.FC<SessionLogsRootProps> = ({
  conversationId,
  view,
  onViewChange,
  children,
}) => {
  const [developerMode] = useConfig('system.developerMode');
  const logsVisible = view === 'logs';
  const [activated, setActivated] = useState(false);
  const [loading, setLoading] = useState(false);
  const [errorKey, setErrorKey] = useState<'loadFailed' | 'developerModeRequired' | null>(null);
  const [entries, setEntries] = useState<ProjectedTurn[]>([]);
  const [summary, setSummary] = useState<ObservationSummary | null>(null);
  const [health, setHealth] = useState<RecorderHealth | null>(null);
  const [selectedId, setSelectedIdState] = useState<string | null>(null);
  const [detail, setDetail] = useState<ProjectedTurn | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [detailErrorKey, setDetailErrorKey] = useState<
    'loadFailed' | 'developerModeRequired' | null
  >(null);
  const [inspectTarget, setInspectTarget] = useState<InspectTarget | null>(null);
  const [callDetail, setCallDetail] = useState<ProjectedModelCall | null>(null);
  const [callLoading, setCallLoading] = useState(false);
  const [callErrorKey, setCallErrorKey] = useState<
    'loadFailed' | 'developerModeRequired' | 'retentionRemoved' | null
  >(null);

  const listSeqRef = useRef(0);
  const turnSeqRef = useRef(0);
  const callSeqRef = useRef(0);
  const selectedIdRef = useRef<string | null>(null);
  const expandedCallIdRef = useRef<string | null>(null);
  const conversationRef = useRef(conversationId);
  const workspaceAbortRef = useRef<AbortController | null>(null);
  const callCacheRef = useRef(new CallDetailLru());
  const pollDelayRef = useRef(POLL_START_MS);
  const lastSeqRef = useRef(0);

  selectedIdRef.current = selectedId;
  expandedCallIdRef.current = inspectTarget?.modelCallId ?? null;
  conversationRef.current = conversationId;

  // Sliding back to dialogue must not abort poll or clear LRU. `activated` stays
  // true after the first open; only a conversation change resets it.
  const live = developerMode === true && activated;

  useEffect(() => {
    setActivated(false);
  }, [conversationId]);

  useEffect(() => {
    if (logsVisible) setActivated(true);
  }, [conversationId, logsVisible]);

  const applyList = useCallback((page: SessionObservationList) => {
    const ordered = page.turns;
    setEntries(ordered);
    setSummary(page.summary);
    setHealth(page.recorder_health);
    if (page.summary.max_event_seq !== lastSeqRef.current) {
      lastSeqRef.current = page.summary.max_event_seq;
      pollDelayRef.current = POLL_START_MS;
    }
    const previousSelected = selectedIdRef.current;
    const nextSelected = resolveSelectedId(ordered, previousSelected);
    const selectedChanged = nextSelected !== previousSelected;
    if (nextSelected == null || selectedChanged) {
      setInspectTarget(null);
      setCallDetail(null);
      setCallErrorKey(null);
      expandedCallIdRef.current = null;
      setDetail(null);
    }
    if (nextSelected == null) {
      setSelectedIdState(null);
    } else {
      setSelectedIdState(nextSelected);
    }
    return { nextSelected, selectedChanged };
  }, []);

  const abortWorkspaceFetches = useCallback(() => {
    workspaceAbortRef.current?.abort();
    workspaceAbortRef.current = null;
  }, []);

  const refreshWorkspace = useCallback(
    async (options?: { signal?: AbortSignal; showListLoading?: boolean }) => {
      const conversation = String(conversationId);
      let signal = options?.signal;
      if (!signal) {
        abortWorkspaceFetches();
        const controller = new AbortController();
        workspaceAbortRef.current = controller;
        signal = controller.signal;
      }
      const listSeq = ++listSeqRef.current;
      if (options?.showListLoading) {
        setLoading(true);
        setErrorKey(null);
      }
      const seqBefore = lastSeqRef.current;
      try {
        const page = await listSessionObservations(conversation, { signal });
        if (signal.aborted || conversationRef.current !== conversationId || listSeq < listSeqRef.current) {
          return { seqChanged: false };
        }
        const { nextSelected, selectedChanged } = applyList(page);
        setErrorKey(null);
        const seqChanged = lastSeqRef.current !== seqBefore;

        if (nextSelected && !selectedChanged) {
          const turnSeq = ++turnSeqRef.current;
          setDetailLoading(true);
          setDetailErrorKey(null);
          try {
            const turn = await getSessionObservationTurn(conversation, nextSelected, { signal });
            if (
              signal.aborted ||
              conversationRef.current !== conversationId ||
              turnSeq < turnSeqRef.current
            ) {
              return { seqChanged };
            }
            setDetail(turn);
            setDetailErrorKey(null);
          } catch (err) {
            if (signal.aborted || isAbortError(err) || turnSeq < turnSeqRef.current) {
              return { seqChanged };
            }
            if (isBackendHttpError(err) && err.status === 403) {
              setDetailErrorKey('developerModeRequired');
            } else {
              setDetailErrorKey('loadFailed');
            }
          } finally {
            if (!signal.aborted && turnSeq >= turnSeqRef.current) setDetailLoading(false);
          }

          const callId = expandedCallIdRef.current;
          if (callId) {
            const cacheKey = callCacheKey(conversation, nextSelected, callId);
            const callSeq = ++callSeqRef.current;
            setCallLoading(true);
            setCallErrorKey(null);
            try {
              const call = await getSessionObservationCall(conversation, nextSelected, callId, {
                signal,
              });
              if (
                signal.aborted ||
                conversationRef.current !== conversationId ||
                callSeq < callSeqRef.current
              ) {
                return { seqChanged };
              }
              callCacheRef.current.set(cacheKey, call);
              setCallDetail(call);
              setCallErrorKey(null);
            } catch (err) {
              if (signal.aborted || isAbortError(err) || callSeq < callSeqRef.current) {
                return { seqChanged };
              }
              if (isObservationRetentionError(err)) {
                callCacheRef.current.delete(cacheKey);
                setCallDetail(null);
                setCallErrorKey('retentionRemoved');
              } else if (isBackendHttpError(err) && err.status === 403) {
                setCallErrorKey('developerModeRequired');
              } else {
                setCallErrorKey('loadFailed');
              }
            } finally {
              if (!signal.aborted && callSeq >= callSeqRef.current) setCallLoading(false);
            }
          }
        }
        return { seqChanged };
      } catch (err) {
        if (signal.aborted || isAbortError(err) || listSeq < listSeqRef.current) {
          return { seqChanged: false };
        }
        if (isBackendHttpError(err) && err.status === 403) {
          setErrorKey('developerModeRequired');
        } else {
          setErrorKey('loadFailed');
        }
        return { seqChanged: false };
      } finally {
        if (!signal.aborted && listSeq >= listSeqRef.current) setLoading(false);
      }
    },
    [abortWorkspaceFetches, applyList, conversationId]
  );

  const closeWorkspace = useCallback(() => {
    onViewChange('dialogue');
  }, [onViewChange]);

  useEffect(() => {
    abortWorkspaceFetches();
    callCacheRef.current.clear();
    setInspectTarget(null);
    setCallDetail(null);
    setEntries([]);
    setSummary(null);
    setHealth(null);
    setSelectedIdState(null);
    setDetail(null);
    setErrorKey(null);
    setDetailErrorKey(null);
    setCallErrorKey(null);
    setLoading(false);
    setDetailLoading(false);
    setCallLoading(false);
    lastSeqRef.current = 0;
    pollDelayRef.current = POLL_START_MS;
    listSeqRef.current += 1;
    turnSeqRef.current += 1;
    callSeqRef.current += 1;
  }, [abortWorkspaceFetches, conversationId]);

  useEffect(() => {
    if (!live) return;
    void refreshWorkspace({ showListLoading: true });
    return () => {
      abortWorkspaceFetches();
    };
  }, [live, conversationId, refreshWorkspace, abortWorkspaceFetches]);

  useEffect(() => {
    if (!logsVisible) return;
    const onKey = (event: KeyboardEvent) => {
      if (shouldCloseWorkspaceOnEscape(event, sessionLogsOverlayOpen())) {
        closeWorkspace();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [closeWorkspace, logsVisible]);

  useEffect(() => {
    if (!live || !selectedId) return;
    const controller = new AbortController();
    const requestSeq = ++turnSeqRef.current;
    setDetailLoading(true);
    setDetailErrorKey(null);
    void getSessionObservationTurn(String(conversationId), selectedId, {
      signal: controller.signal,
    })
      .then((turn) => {
        if (controller.signal.aborted || requestSeq < turnSeqRef.current) return;
        setDetail(turn);
        setDetailErrorKey(null);
      })
      .catch((err) => {
        if (controller.signal.aborted || isAbortError(err) || requestSeq < turnSeqRef.current) {
          return;
        }
        setDetail((current) => (current?.root_turn_id === selectedId ? current : null));
        if (isBackendHttpError(err) && err.status === 403) {
          setDetailErrorKey('developerModeRequired');
        } else {
          setDetailErrorKey('loadFailed');
        }
      })
      .finally(() => {
        if (!controller.signal.aborted && requestSeq >= turnSeqRef.current) {
          setDetailLoading(false);
        }
      });
    return () => {
      controller.abort();
    };
  }, [live, selectedId, conversationId]);

  const expandedCallId = inspectTarget?.modelCallId ?? null;
  const selectedTurnEnded =
    detail?.root_turn_id === selectedId
      ? detail.has_turn_end === true
      : entries.some((turn) => turn.root_turn_id === selectedId && turn.has_turn_end === true);

  useEffect(() => {
    if (!live || !selectedId || !expandedCallId) return;
    const cacheKey = callCacheKey(String(conversationId), selectedId, expandedCallId);
    const cached = callCacheRef.current.get(cacheKey);
    if (cached) {
      setCallDetail(cached);
      setCallErrorKey(null);
      setCallLoading(false);
      if (selectedTurnEnded) return;
    }
    const controller = new AbortController();
    const requestSeq = ++callSeqRef.current;
    if (!cached) {
      setCallLoading(true);
      setCallErrorKey(null);
      setCallDetail(null);
    }
    void getSessionObservationCall(String(conversationId), selectedId, expandedCallId, {
      signal: controller.signal,
    })
      .then((call) => {
        if (controller.signal.aborted || requestSeq < callSeqRef.current) return;
        callCacheRef.current.set(cacheKey, call);
        setCallDetail(call);
        setCallErrorKey(null);
      })
      .catch((err) => {
        if (controller.signal.aborted || isAbortError(err) || requestSeq < callSeqRef.current) {
          return;
        }
        if (isObservationRetentionError(err)) {
          callCacheRef.current.delete(cacheKey);
          setCallDetail(null);
          setCallErrorKey('retentionRemoved');
        } else if (isBackendHttpError(err) && err.status === 403) {
          if (!cached) setCallDetail(null);
          setCallErrorKey('developerModeRequired');
        } else {
          if (!cached) setCallDetail(null);
          setCallErrorKey('loadFailed');
        }
      })
      .finally(() => {
        if (!controller.signal.aborted && requestSeq >= callSeqRef.current) {
          setCallLoading(false);
        }
      });
    return () => {
      controller.abort();
    };
  }, [live, selectedId, expandedCallId, conversationId, selectedTurnEnded]);

  const shouldPoll = shouldPollTurns(entries, health);

  useEffect(() => {
    if (!live || !shouldPoll) return;
    const controller = new AbortController();
    let cancelled = false;
    let timeoutId = 0;
    const schedule = () => {
      timeoutId = window.setTimeout(() => {
        const seqBefore = lastSeqRef.current;
        void refreshWorkspace({ signal: controller.signal })
          .catch(() => undefined)
          .finally(() => {
            if (cancelled) return;
            if (lastSeqRef.current === seqBefore) {
              pollDelayRef.current = nextPollDelay(pollDelayRef.current);
            }
            schedule();
          });
      }, pollDelayRef.current);
    };
    schedule();
    return () => {
      cancelled = true;
      controller.abort();
      window.clearTimeout(timeoutId);
    };
  }, [conversationId, live, refreshWorkspace, shouldPoll]);

  const setSelectedId = useCallback((id: string) => {
    setSelectedIdState(id);
    setInspectTarget(null);
    setCallDetail(null);
    setDetail(null);
    setDetailErrorKey(null);
    setCallErrorKey(null);
  }, []);

  const value = useMemo<SessionLogsContextValue>(
    () => ({
      conversationId,
      view,
      onViewChange,
      developerMode: developerMode === true,
      loading,
      errorKey,
      entries,
      summary,
      health,
      selectedId,
      setSelectedId,
      detail,
      detailLoading,
      detailErrorKey,
      inspectTarget,
      setInspectTarget,
      callDetail,
      callLoading,
      callErrorKey,
      refreshWorkspace,
    }),
    [
      conversationId,
      view,
      onViewChange,
      developerMode,
      loading,
      errorKey,
      entries,
      summary,
      health,
      selectedId,
      setSelectedId,
      detail,
      detailLoading,
      detailErrorKey,
      inspectTarget,
      callDetail,
      callLoading,
      callErrorKey,
      refreshWorkspace,
    ]
  );

  return <SessionLogsContext.Provider value={value}>{children}</SessionLogsContext.Provider>;
};

export const AgentTraceTrigger: React.FC = () => {
  const { t } = useTranslation();
  const { view, onViewChange, developerMode } = useSessionLogs();
  if (developerMode !== true) return null;
  const open = view === 'logs';
  return (
    <Tooltip content={t('conversation.agentTrace.openInspector')}>
      <Button
        type='text'
        size='mini'
        shape='round'
        className={classNames(capabilityHeaderButtonClass(open), 'flowy-icon-text-btn')}
        style={capabilityHeaderButtonStyle(OBSERVE_ACCENT)}
        icon={<Bug theme='outline' size='14' strokeWidth={3} />}
        onClick={() => onViewChange(open ? 'dialogue' : 'logs')}
        aria-label={t('conversation.agentTrace.openInspector')}
        aria-pressed={open}
      >
        {t('conversation.agentTrace.openInspector')}
      </Button>
    </Tooltip>
  );
};

export const SessionLogWorkspace: React.FC = () => {
  const { t } = useTranslation();
  const {
    developerMode,
    loading,
    errorKey,
    entries,
    summary,
    health,
    selectedId,
    setSelectedId,
    detail,
    detailLoading,
    detailErrorKey,
    inspectTarget,
    setInspectTarget,
    callDetail,
    callLoading,
    callErrorKey,
    refreshWorkspace,
  } = useSessionLogs();
  const [newestFirst, setNewestFirst] = useState(true);
  const rounds = useMemo(() => assignTurnRounds(entries), [entries]);
  const displayed = useMemo(
    () => (newestFirst ? [...entries].reverse() : entries),
    [entries, newestFirst]
  );

  const retryWorkspace = () => void refreshWorkspace({ showListLoading: true });
  const errorMessage = errorKey ? t(`conversation.agentTrace.${errorKey}`) : null;
  const retryControl = (
    <Button
      type='text'
      size='mini'
      className='session-logs-retry'
      onClick={retryWorkspace}
      disabled={loading}
    >
      {t('conversation.agentTrace.refresh')}
    </Button>
  );

  if (developerMode !== true) return null;

  const healthStatus = health?.status ?? 'healthy';
  const healthFault = healthIsFault(health);
  const healthUnhealthy = health != null && health.status !== 'healthy';

  return (
    <div
      className='session-logs-workspace'
      role='region'
      aria-label={t('conversation.agentTrace.workspaceTitle')}
    >
      {loading && entries.length === 0 ? (
        <div className='flex justify-center py-40px'>
          <Spin />
        </div>
      ) : entries.length === 0 ? (
        <div className='session-logs-empty'>
          {errorMessage ? (
            <>
              <div className='px-16px pt-24px text-13px text-[var(--color-text-2)]'>{errorMessage}</div>
              <div className='flex justify-center pt-8px'>{retryControl}</div>
            </>
          ) : (
            <Empty className='py-40px' description={t('conversation.agentTrace.empty')} />
          )}
        </div>
      ) : (
        <div className='session-logs-body'>
          <div className='session-logs-nav'>
            <div className='session-logs-nav__header'>
              <div className='session-logs-nav__toolbar'>
                <Popover
                  trigger={['click', 'hover']}
                  position='bl'
                  content={
                    <dl className='session-logs-glossary'>
                      <dt>{t('conversation.agentTrace.metricTurn')}</dt>
                      <dd>{t('conversation.agentTrace.metricTurnHint')}</dd>
                      <dt>{t('conversation.agentTrace.metricModel')}</dt>
                      <dd>{t('conversation.agentTrace.metricModelHint')}</dd>
                      <dt>{t('conversation.agentTrace.metricTool')}</dt>
                      <dd>{t('conversation.agentTrace.metricToolHint')}</dd>
                      <dt>{t('conversation.agentTrace.metricDuration')}</dt>
                      <dd>{t('conversation.agentTrace.metricDurationHint')}</dd>
                      <dt>{t('conversation.agentTrace.glossaryIntegrity')}</dt>
                      <dd>{t('conversation.agentTrace.integrityHint')}</dd>
                    </dl>
                  }
                >
                  <button
                    type='button'
                    className='session-logs-info'
                    aria-label={t('conversation.agentTrace.glossaryAria')}
                  >
                    <Info theme='outline' size='14' strokeWidth={3} />
                  </button>
                </Popover>
                <div className='session-logs-nav__toolbar-actions'>
                  <Tooltip
                    content={
                      newestFirst
                        ? t('conversation.agentTrace.newestFirst')
                        : t('conversation.agentTrace.oldestFirst')
                    }
                  >
                    <Button
                      type='text'
                      size='mini'
                      className='session-logs-json-tree__icon-btn'
                      icon={
                        newestFirst ? (
                          <SortAmountDown theme='outline' size='14' strokeWidth={3} />
                        ) : (
                          <SortAmountUp theme='outline' size='14' strokeWidth={3} />
                        )
                      }
                      aria-pressed={newestFirst}
                      aria-label={
                        newestFirst
                          ? t('conversation.agentTrace.newestFirst')
                          : t('conversation.agentTrace.oldestFirst')
                      }
                      onClick={() => setNewestFirst((value) => !value)}
                    />
                  </Tooltip>
                  <Tooltip content={t('conversation.agentTrace.refresh')}>
                    <Button
                      type='text'
                      size='mini'
                      className='session-logs-json-tree__icon-btn'
                      icon={<Refresh theme='outline' size='14' strokeWidth={3} />}
                      onClick={() => void refreshWorkspace({ showListLoading: true })}
                      disabled={loading}
                      aria-label={t('conversation.agentTrace.refresh')}
                    />
                  </Tooltip>
                </div>
              </div>
              {summary ? (
                <div className='session-logs-metrics'>
                  <MetricStat
                    value={summary.turn_count}
                    label={t('conversation.agentTrace.metricTurn')}
                  />
                  <MetricStat
                    value={summary.model_call_count}
                    label={t('conversation.agentTrace.metricModel')}
                  />
                  <MetricStat
                    value={summary.tool_count}
                    label={t('conversation.agentTrace.metricTool')}
                  />
                  <MetricStat
                    value={
                      formatDurationMs(summary.active_duration_ms) ||
                      t('conversation.agentTrace.activeDuration')
                    }
                    label={t('conversation.agentTrace.metricDuration')}
                  />
                </div>
              ) : null}
              {errorMessage ? (
                <div className='session-logs-health session-logs-health--fault'>
                  {errorMessage}
                </div>
              ) : null}
              {healthUnhealthy ? (
                <div
                  className={classNames(
                    'session-logs-health',
                    healthFault && 'session-logs-health--fault'
                  )}
                >
                  {t('conversation.agentTrace.writerHealth')}
                  {' · '}
                  {t(`conversation.agentTrace.health_${healthStatus}`)}
                  {health?.last_error ? ` · ${health.last_error}` : ''}
                </div>
              ) : null}
            </div>
            <div className='session-logs-nav__list'>
              {displayed.map((entry) => {
                const active = entry.root_turn_id === selectedId;
                const round = rounds.get(entry.root_turn_id) ?? 0;
                const toolCount = turnToolCount(entry);
                const elapsed = formatDurationMs(entry.elapsed_ms);
                const clock = formatClock(entry.started_at_ms);
                return (
                  <button
                    key={entry.root_turn_id}
                    type='button'
                    className={classNames('session-logs-nav__item', active && 'is-active')}
                    onClick={() => setSelectedId(entry.root_turn_id)}
                  >
                    <div className='flex items-baseline justify-between gap-8px'>
                      <span className='session-logs-nav__round'>
                        {t('conversation.agentTrace.roundLabel', { n: round })}
                      </span>
                      {clock ? (
                        <time className='session-logs-nav__time'>{clock}</time>
                      ) : null}
                    </div>
                    <div className='session-logs-nav__prompt'>
                      {entry.prompt_preview || t('conversation.agentTrace.previewMissing')}
                    </div>
                    <div className='session-logs-nav__counts'>
                      {t('conversation.agentTrace.modelCallCount', {
                        count: entry.model_calls.length,
                      })}
                      {' · '}
                      {t('conversation.agentTrace.toolCallCount', { count: toolCount })}
                      {elapsed ? ` · ${elapsed}` : ''}
                      {entry.status
                        ? ` · ${t(`conversation.agentTrace.status_${entry.status}`)}`
                        : ''}
                    </div>
                    {entry.interrupted || entry.gap_count > 0 || entry.integrity === 'degraded' ? (
                      <div className='session-logs-nav__flags'>
                        {entry.interrupted ? (
                          <span className='session-logs-flag'>
                            {t('conversation.agentTrace.interrupted')}
                          </span>
                        ) : null}
                        {entry.gap_count > 0 ? (
                          <span className='session-logs-flag'>
                            {t('conversation.agentTrace.gapCount', { count: entry.gap_count })}
                          </span>
                        ) : null}
                        {entry.integrity === 'degraded' ? (
                          <span className='session-logs-flag'>
                            {t('conversation.agentTrace.integrityDegraded')}
                          </span>
                        ) : null}
                      </div>
                    ) : null}
                  </button>
                );
              })}
            </div>
          </div>

          <div className='session-logs-detail'>
            {detailLoading && !detail ? (
              <div className='flex justify-center py-24px'>
                <Spin />
              </div>
            ) : detail ? (
              <ObservationWorkflow
                turn={detail}
                inspectTarget={inspectTarget}
                callDetail={callDetail}
                callLoading={callLoading}
                callErrorKey={callErrorKey}
                onInspect={setInspectTarget}
              />
            ) : detailErrorKey ? (
              <div className='px-16px py-24px text-13px text-[var(--color-text-2)]'>
                {t(`conversation.agentTrace.${detailErrorKey}`)}
              </div>
            ) : (
              <Empty className='py-24px' description={t('conversation.agentTrace.empty')} />
            )}
          </div>
        </div>
      )}
    </div>
  );
};

const AgentTraceInspector = AgentTraceTrigger;
export default AgentTraceInspector;
