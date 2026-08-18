/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useCallback, useEffect, useState } from 'react';
import classNames from 'classnames';
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
import ObservationWorkflow from './ObservationWorkflow';
import { shortId } from './format';
import {
  getSessionObservationTurn,
  listSessionObservations,
  type ProjectedTurn,
} from './useAgentTraces';

export interface AgentTraceInspectorProps {
  conversationId: ConversationId;
}

const ACCENT = 'var(--color-text-2)';

/**
 * Developer Mode–gated drawer: left turns, right REQUEST → RESPONSE → tools.
 * Renders persisted observation projections only — never chat bubbles.
 */
export const AgentTraceInspector: React.FC<AgentTraceInspectorProps> = ({ conversationId }) => {
  const { t } = useTranslation();
  const [developerMode] = useConfig('system.developerMode');
  const [open, setOpen] = useState(false);
  const [loading, setLoading] = useState(false);
  const [errorKey, setErrorKey] = useState<'loadFailed' | 'developerModeRequired' | null>(null);
  const [entries, setEntries] = useState<ProjectedTurn[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [detail, setDetail] = useState<ProjectedTurn | null>(null);
  const [detailLoading, setDetailLoading] = useState(false);

  const loadList = useCallback(async () => {
    setLoading(true);
    setErrorKey(null);
    try {
      const rows = await listSessionObservations(String(conversationId));
      const ordered = [...rows].reverse();
      setEntries(ordered);
      if (ordered.length === 0) {
        setSelectedId(null);
        setDetail(null);
      } else {
        setSelectedId((current) =>
          current && ordered.some((row) => row.root_turn_id === current)
            ? current
            : ordered[0].root_turn_id
        );
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
  }, [conversationId]);

  useEffect(() => {
    if (!open || developerMode !== true) return;
    void loadList();
  }, [open, conversationId, developerMode, loadList]);

  useEffect(() => {
    if (!open || !selectedId || developerMode !== true) return;
    let cancelled = false;
    setDetailLoading(true);
    void getSessionObservationTurn(String(conversationId), selectedId)
      .then((turn) => {
        if (!cancelled) setDetail(turn);
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
  }, [open, selectedId, developerMode, conversationId]);

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
          className={classNames(capabilityHeaderButtonClass(open), 'flowy-icon-text-btn')}
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
            className='flowy-icon-text-btn'
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
            <div className='max-h-[26%] overflow-auto border-b border-solid border-[var(--color-border-2)] shrink-0'>
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
                    onClick={() => setSelectedId(entry.root_turn_id)}
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
                          {entry.session_kind ?? t('conversation.agentTrace.sessionKind')}
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

            <div className='flex-1 overflow-auto min-h-0'>
              {detailLoading && !detail ? (
                <div className='flex justify-center py-24px'>
                  <Spin />
                </div>
              ) : detail ? (
                <ObservationWorkflow turn={detail} />
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
