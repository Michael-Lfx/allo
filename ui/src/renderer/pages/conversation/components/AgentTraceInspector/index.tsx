/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, Drawer, Empty, Spin, Tag, Tooltip } from '@arco-design/web-react';
import { Bug, Refresh } from '@icon-park/react';
import { isBackendHttpError } from '@/common/adapter/httpBridge';
import type { ConversationId } from '@/common/types/ids';
import { useConfig } from '@/renderer/hooks/config/useConfig';
import {
  capabilityHeaderButtonClass,
  capabilityHeaderButtonStyle,
} from '../CapabilityHeaderButton';
import TraceTimeline from './TraceTimeline';
import TraceTurnSummary from './TraceTurnSummary';
import {
  formatClock,
  formatElapsed,
  formatTokenCount,
  outcomeLabel,
  shortId,
} from './format';
import {
  getAgentTrace,
  listAgentTraces,
  type AgentTraceIndexEntry,
  type AgentTurnTrace,
} from './useAgentTraces';

export interface AgentTraceInspectorProps {
  conversationId: ConversationId;
}

const ACCENT = 'var(--color-text-2)';

/**
 * Developer Mode–gated drawer that lists turn traces for the current
 * conversation and shows detailed span / metrics for a selected turn.
 */
export const AgentTraceInspector: React.FC<AgentTraceInspectorProps> = ({ conversationId }) => {
  const { t } = useTranslation();
  const [developerMode] = useConfig('system.developerMode');
  const [open, setOpen] = useState(false);
  const [loading, setLoading] = useState(false);
  const [errorKey, setErrorKey] = useState<'loadFailed' | 'developerModeRequired' | null>(null);
  const [entries, setEntries] = useState<AgentTraceIndexEntry[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [detail, setDetail] = useState<AgentTurnTrace | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);

  const loadList = useCallback(async () => {
    setLoading(true);
    setErrorKey(null);
    try {
      const rows = await listAgentTraces(String(conversationId), 100);
      setEntries(rows);
      if (rows.length === 0) {
        setSelectedId(null);
        setDetail(null);
      } else if (!selectedId || !rows.some((r) => r.trace_id === selectedId)) {
        setSelectedId(rows[0].trace_id);
      }
    } catch (err) {
      setEntries([]);
      setDetail(null);
      setSelectedId(null);
      if (isBackendHttpError(err) && err.status === 403) {
        setErrorKey('developerModeRequired');
      } else {
        setErrorKey('loadFailed');
      }
    } finally {
      setLoading(false);
    }
  }, [conversationId, selectedId]);

  useEffect(() => {
    if (!open || developerMode !== true) return;
    void loadList();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- refresh on open / conversation change only
  }, [open, conversationId, developerMode]);

  useEffect(() => {
    if (!open || !selectedId || developerMode !== true) return;
    let cancelled = false;
    setDetailLoading(true);
    void getAgentTrace(selectedId)
      .then((trace) => {
        if (!cancelled) setDetail(trace);
      })
      .catch(() => {
        if (!cancelled) setDetail(null);
      })
      .finally(() => {
        if (!cancelled) setDetailLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open, selectedId, developerMode]);

  if (developerMode !== true) {
    return null;
  }

  return (
    <>
      <Tooltip content={t('conversation.agentTrace.openInspector')}>
        <Button
          type='text'
          size='mini'
          shape='round'
          className={capabilityHeaderButtonClass(open)}
          style={capabilityHeaderButtonStyle(ACCENT)}
          icon={<Bug theme='outline' size='14' strokeWidth={3} />}
          onClick={() => setOpen(true)}
          aria-label={t('conversation.agentTrace.openInspector')}
        >
          {t('conversation.agentTrace.openInspector')}
        </Button>
      </Tooltip>

      <Drawer
        visible={open}
        width='min(760px, calc(100vw - 12px))'
        placement='right'
        title={t('conversation.agentTrace.title')}
        footer={null}
        focusLock
        autoFocus={false}
        getPopupContainer={() => document.body}
        onCancel={() => setOpen(false)}
        bodyStyle={{ padding: 0 }}
      >
        <div className='flex items-center justify-between gap-8px px-12px py-8px border-b border-solid border-[var(--color-border-2)]'>
          <div className='min-w-0'>
            <div className='text-11px text-[var(--color-text-3)] truncate'>{conversationId}</div>
            <div className='text-10px text-[var(--color-text-4)]'>
              {t('conversation.agentTrace.turnCount', { count: entries.length })}
            </div>
          </div>
          <Button
            type='text'
            size='mini'
            icon={<Refresh theme='outline' size='14' strokeWidth={3} />}
            onClick={() => void loadList()}
            disabled={loading}
          >
            {t('conversation.agentTrace.refresh')}
          </Button>
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
          <div className='flex flex-col min-h-0' style={{ height: 'calc(100% - 49px)' }}>
            <div className='max-h-[30%] overflow-auto border-b border-solid border-[var(--color-border-2)] shrink-0'>
              {entries.map((entry) => {
                const active = entry.trace_id === selectedId;
                const outcome = outcomeLabel(entry.success, entry.stop_reason);
                return (
                  <button
                    key={entry.trace_id}
                    type='button'
                    className='w-full text-left px-12px py-8px border-0 border-b border-solid border-[var(--color-border-1)] cursor-pointer'
                    style={{
                      background: active
                        ? 'color-mix(in srgb, var(--color-text-2) 8%, var(--color-bg-1))'
                        : 'transparent',
                    }}
                    onClick={() => setSelectedId(entry.trace_id)}
                  >
                    <div className='flex items-center justify-between gap-8px'>
                      <div className='flex items-center gap-6px min-w-0'>
                        {outcome === 'fail' ? (
                          <Tag size='small' color='red'>
                            fail
                          </Tag>
                        ) : outcome === 'cancelled' ? (
                          <Tag size='small' color='orangered'>
                            cancel
                          </Tag>
                        ) : outcome === 'ok' ? (
                          <Tag size='small' color='green'>
                            ok
                          </Tag>
                        ) : (
                          <Tag size='small' color='gray'>
                            —
                          </Tag>
                        )}
                        <span className='text-12px font-600 text-[var(--color-text-1)] truncate'>
                          {entry.session_kind}
                        </span>
                        {entry.stop_reason ? (
                          <span className='text-10px text-[var(--color-text-3)] truncate'>
                            {entry.stop_reason}
                          </span>
                        ) : null}
                      </div>
                      <span className='text-11px text-[var(--color-text-3)] tabular-nums shrink-0'>
                        {formatElapsed(entry.elapsed_ms)}
                      </span>
                    </div>
                    <div className='text-11px text-[var(--color-text-3)] mt-3px flex gap-x-10px gap-y-2px flex-wrap'>
                      <span>{formatClock(entry.started_at_ms)}</span>
                      <span>
                        {t('conversation.agentTrace.tokens')}:{' '}
                        {formatTokenCount(entry.input_tokens)}→
                        {formatTokenCount(entry.output_tokens)}
                      </span>
                      <span>
                        {t('conversation.agentTrace.tools')}: {entry.tool_call_count}
                        {entry.tool_error_count > 0
                          ? ` · ${entry.tool_error_count} err`
                          : ''}
                      </span>
                      <span className='font-mono' title={entry.msg_id}>
                        msg={shortId(entry.msg_id)}
                      </span>
                      <span className='font-mono' title={entry.trace_id}>
                        trace={shortId(entry.trace_id)}
                      </span>
                    </div>
                  </button>
                );
              })}
            </div>

            <div className='flex-1 overflow-auto min-h-0'>
              {detailLoading && !detail ? (
                <div className='flex justify-center py-24px'>
                  <Spin />
                </div>
              ) : detail ? (
                <>
                  <TraceTurnSummary trace={detail} />
                  <TraceTimeline spans={detail.spans ?? []} turnStartedAtMs={detail.started_at_ms} />
                </>
              ) : (
                <Empty className='py-24px' description={t('conversation.agentTrace.empty')} />
              )}
            </div>
          </div>
        )}
      </Drawer>
    </>
  );
};

export default AgentTraceInspector;
