/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useCallback, useEffect, useRef, useState } from 'react';
import classNames from 'classnames';
import { useTranslation } from 'react-i18next';
import { Button, Empty, Spin, Tag, Tooltip } from '@arco-design/web-react';
import { Bug, Close, Refresh } from '@icon-park/react';
import { isBackendHttpError } from '@/common/adapter/httpBridge';
import type { ConversationId } from '@/common/types/ids';
import { useConfig } from '@/renderer/hooks/config/useConfig';
import {
  capabilityHeaderButtonClass,
  capabilityHeaderButtonStyle,
} from '../CapabilityHeaderButton';
import { CallDetailLru, callCacheKey } from './callDetailCache';
import ObservationWorkflow from './ObservationWorkflow';
import { shortId } from './format';
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

export interface AgentTraceInspectorProps {
  conversationId: ConversationId;
}

const ACCENT = 'var(--color-text-2)';
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

function resolveSelectedId(ordered: ProjectedTurn[], current: string | null): string | null {
  if (ordered.length === 0) return null;
  if (current && ordered.some((row) => row.root_turn_id === current)) return current;
  return ordered[0].root_turn_id;
}

/**
 * Developer Mode–gated workspace overlay: navigator + summary + lazy Call GET.
 * Renders persisted observation projections only — never chat bubbles.
 */
export const AgentTraceInspector: React.FC<AgentTraceInspectorProps> = ({ conversationId }) => {
  const { t } = useTranslation();
  const [developerMode] = useConfig('system.developerMode');
  const [open, setOpen] = useState(false);
  const [loading, setLoading] = useState(false);
  const [errorKey, setErrorKey] = useState<'loadFailed' | 'developerModeRequired' | null>(null);
  const [entries, setEntries] = useState<ProjectedTurn[]>([]);
  const [summary, setSummary] = useState<ObservationSummary | null>(null);
  const [health, setHealth] = useState<RecorderHealth | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [detail, setDetail] = useState<ProjectedTurn | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);
  const [detailErrorKey, setDetailErrorKey] = useState<
    'loadFailed' | 'developerModeRequired' | null
  >(null);
  const [expandedCallId, setExpandedCallId] = useState<string | null>(null);
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
  expandedCallIdRef.current = expandedCallId;
  conversationRef.current = conversationId;

  const applyList = useCallback((page: SessionObservationList) => {
    const ordered = [...page.turns].reverse();
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
      setExpandedCallId(null);
      setCallDetail(null);
      setCallErrorKey(null);
      expandedCallIdRef.current = null;
    }
    if (nextSelected == null) {
      setSelectedId(null);
      setDetail(null);
    } else {
      setSelectedId(nextSelected);
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
      if (options?.showListLoading) setLoading(true);
      setErrorKey(null);
      const seqBefore = lastSeqRef.current;
      try {
        const page = await listSessionObservations(conversation, { signal });
        if (signal.aborted || conversationRef.current !== conversationId || listSeq < listSeqRef.current) {
          return { seqChanged: false };
        }
        const { nextSelected, selectedChanged } = applyList(page);
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
            setDetail(null);
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
              setCallDetail(null);
              if (isObservationRetentionError(err)) {
                callCacheRef.current.delete(cacheKey);
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
        setEntries([]);
        setSummary(null);
        setHealth(null);
        setDetail(null);
        setSelectedId(null);
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
    abortWorkspaceFetches();
    setOpen(false);
    setExpandedCallId(null);
    setCallDetail(null);
    callCacheRef.current.clear();
  }, [abortWorkspaceFetches]);

  useEffect(() => {
    abortWorkspaceFetches();
    callCacheRef.current.clear();
    setExpandedCallId(null);
    setCallDetail(null);
    setEntries([]);
    setSummary(null);
    setHealth(null);
    setSelectedId(null);
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
    if (!open || developerMode !== true) return;
    void refreshWorkspace({ showListLoading: true });
    return () => {
      abortWorkspaceFetches();
    };
  }, [open, conversationId, developerMode, refreshWorkspace, abortWorkspaceFetches]);

  useEffect(() => {
    if (!open) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') closeWorkspace();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [closeWorkspace, open]);

  useEffect(() => {
    if (!open || !selectedId || developerMode !== true) return;
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
        setDetail(null);
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
  }, [open, selectedId, developerMode, conversationId]);

  useEffect(() => {
    if (!open || !selectedId || !expandedCallId || developerMode !== true) return;
    const cacheKey = callCacheKey(String(conversationId), selectedId, expandedCallId);
    const cached = callCacheRef.current.get(cacheKey);
    if (cached) {
      setCallDetail(cached);
      setCallErrorKey(null);
      setCallLoading(false);
      return;
    }
    const controller = new AbortController();
    const requestSeq = ++callSeqRef.current;
    setCallLoading(true);
    setCallErrorKey(null);
    setCallDetail(null);
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
        setCallDetail(null);
        if (isObservationRetentionError(err)) {
          callCacheRef.current.delete(cacheKey);
          setCallErrorKey('retentionRemoved');
        } else if (isBackendHttpError(err) && err.status === 403) {
          setCallErrorKey('developerModeRequired');
        } else {
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
  }, [open, selectedId, expandedCallId, developerMode, conversationId]);

  const shouldPoll = shouldPollTurns(entries, health);

  useEffect(() => {
    if (!open || developerMode !== true || !shouldPoll) return;
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
  }, [conversationId, developerMode, open, refreshWorkspace, shouldPoll]);

  if (developerMode !== true) {
    return null;
  }

  const healthStatus = health?.status ?? 'healthy';

  return (
    <>
      <Tooltip content={t('conversation.agentTrace.openInspector')}>
        <Button
          type='text'
          size='mini'
          shape='round'
          className={classNames(capabilityHeaderButtonClass(open), 'flowy-icon-text-btn')}
          style={capabilityHeaderButtonStyle(ACCENT)}
          icon={<Bug theme='outline' size='14' strokeWidth={3} />}
          onClick={() => setOpen(true)}
          aria-label={t('conversation.agentTrace.openInspector')}
        >
          {t('conversation.agentTrace.openInspector')}
        </Button>
      </Tooltip>

      {open ? (
        <div
          className='fixed inset-0 z-1000 flex flex-col bg-[var(--color-bg-1)]'
          role='dialog'
          aria-modal='true'
          aria-label={t('conversation.agentTrace.workspaceTitle')}
        >
          <div className='flex items-start justify-between gap-12px px-16px py-10px border-b border-solid border-[var(--color-border-2)]'>
            <div className='min-w-0 flex-1'>
              <div className='flex items-center gap-8px'>
                <div className='text-14px font-600 text-[var(--color-text-1)]'>
                  {t('conversation.agentTrace.workspaceTitle')}
                </div>
                <span className='text-11px text-[var(--color-text-3)] truncate'>{conversationId}</span>
              </div>
              <div className='mt-6px text-12px text-[var(--color-text-2)]'>
                {t('conversation.agentTrace.writerHealth')}
                {' · '}
                {t(`conversation.agentTrace.health_${healthStatus}`)}
                {health?.last_error ? ` · ${health.last_error}` : ''}
              </div>
              <div className='mt-2px text-12px text-[var(--color-text-2)]'>
                {t('conversation.agentTrace.sessionLog')}
                {' · '}
                {summary
                  ? t(
                      summary.integrity === 'degraded'
                        ? 'conversation.agentTrace.integrityDegraded'
                        : 'conversation.agentTrace.integrityComplete'
                    )
                  : '—'}
                {summary ? ` · ${t('conversation.agentTrace.coverageRetained')}` : ''}
                {summary
                  ? ` · ${t('conversation.agentTrace.turnCount', { count: summary.turn_count })} · ${t('conversation.agentTrace.activeDuration')} ${summary.active_duration_ms}ms`
                  : ''}
              </div>
            </div>
            <div className='flex items-center gap-4px shrink-0'>
              <Button
                type='text'
                size='mini'
                className='flowy-icon-text-btn'
                icon={<Refresh theme='outline' size='14' strokeWidth={3} />}
                onClick={() => void refreshWorkspace({ showListLoading: true })}
                disabled={loading}
              >
                {t('conversation.agentTrace.refresh')}
              </Button>
              <Button
                type='text'
                size='mini'
                className='flowy-icon-text-btn'
                icon={<Close theme='outline' size='14' strokeWidth={3} />}
                onClick={closeWorkspace}
                aria-label={t('conversation.agentTrace.closeWorkspace')}
              />
            </div>
          </div>

          {loading && entries.length === 0 ? (
            <div className='flex justify-center py-40px'>
              <Spin />
            </div>
          ) : errorKey ? (
            <div className='px-16px py-24px text-13px text-[var(--color-text-2)]'>
              {t(`conversation.agentTrace.${errorKey}`)}
            </div>
          ) : entries.length === 0 ? (
            <Empty className='py-40px' description={t('conversation.agentTrace.empty')} />
          ) : (
            <div className='flex min-h-0 flex-1'>
              <div className='w-280px shrink-0 overflow-auto border-r border-solid border-[var(--color-border-2)]'>
                {entries.map((entry) => {
                  const active = entry.root_turn_id === selectedId;
                  const degraded = entry.integrity === 'degraded';
                  return (
                    <button
                      key={entry.root_turn_id}
                      type='button'
                      className='w-full text-left px-12px py-8px border-0 border-b border-solid border-[var(--color-border-1)] cursor-pointer'
                      style={{
                        background: active
                          ? 'color-mix(in srgb, var(--color-text-2) 8%, var(--color-bg-1))'
                          : 'transparent',
                      }}
                      onClick={() => {
                        setSelectedId(entry.root_turn_id);
                        setExpandedCallId(null);
                        setCallDetail(null);
                      }}
                    >
                      <div className='flex items-center justify-between gap-8px'>
                        <div className='flex items-center gap-6px min-w-0'>
                          <Tag size='small' color={degraded ? 'orangered' : 'green'}>
                            {degraded
                              ? t('conversation.agentTrace.integrityDegraded')
                              : t('conversation.agentTrace.integrityComplete')}
                          </Tag>
                          {entry.interrupted ? (
                            <Tag size='small' color='orange'>
                              {t('conversation.agentTrace.interrupted')}
                            </Tag>
                          ) : null}
                          <span className='text-12px font-600 text-[var(--color-text-1)] truncate'>
                            {entry.prompt_preview ||
                              entry.session_kind ||
                              t('conversation.agentTrace.previewMissing')}
                          </span>
                        </div>
                        <span className='text-11px text-[var(--color-text-3)] tabular-nums shrink-0'>
                          {t('conversation.agentTrace.modelCalls')}: {entry.model_calls.length}
                        </span>
                      </div>
                      <div className='text-11px text-[var(--color-text-3)] mt-3px flex gap-x-10px gap-y-2px flex-wrap'>
                        <span>
                          {t('conversation.agentTrace.tools')}:{' '}
                          {entry.model_calls.reduce((sum, call) => sum + call.tools.length, 0)}
                        </span>
                        {entry.gap_count > 0 ? (
                          <span>
                            {t('conversation.agentTrace.gap')}: {entry.gap_count}
                          </span>
                        ) : null}
                        <span className='font-mono' title={entry.msg_id ?? undefined}>
                          msg={shortId(entry.msg_id)}
                        </span>
                        <span className='font-mono' title={entry.root_turn_id}>
                          turn={shortId(entry.root_turn_id)}
                        </span>
                      </div>
                    </button>
                  );
                })}
              </div>

              <div className='flex-1 min-w-0 min-h-0'>
                {detailLoading && !detail ? (
                  <div className='flex justify-center py-24px'>
                    <Spin />
                  </div>
                ) : detailErrorKey ? (
                  <div className='px-16px py-24px text-13px text-[var(--color-text-2)]'>
                    {t(`conversation.agentTrace.${detailErrorKey}`)}
                  </div>
                ) : detail ? (
                  <ObservationWorkflow
                    turn={detail}
                    expandedCallId={expandedCallId}
                    callDetail={callDetail}
                    callLoading={callLoading}
                    callErrorKey={callErrorKey}
                    onToggleCall={(modelCallId) => {
                      setExpandedCallId((current) => {
                        if (current === modelCallId) {
                          setCallDetail(null);
                          return null;
                        }
                        return modelCallId;
                      });
                    }}
                  />
                ) : (
                  <Empty className='py-24px' description={t('conversation.agentTrace.empty')} />
                )}
              </div>
            </div>
          )}
        </div>
      ) : null}
    </>
  );
};

export default AgentTraceInspector;
