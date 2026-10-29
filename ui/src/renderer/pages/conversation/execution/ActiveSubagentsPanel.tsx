/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { ipcBridge } from '@/common';
import type { TActiveSubagent } from '@/common/types/agentExecution/agentExecutionTypes';
import type { ExecutionId } from '@/common/types/ids';
import React, { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';

const REFRESH_DEBOUNCE_MS = 150;
const LIMIT = 20;

function statusDotClass(status: TActiveSubagent['attempt_status']): string {
  if (status === 'running') return 'bg-primary-6';
  if (status === 'waiting_input') return 'bg-warning-6';
  return 'bg-t-tertiary';
}

/**
 * Read-only projection of active delegated Agent invocations. The backend owns
 * the filtering rules; this component only presents the flattened rows and
 * refreshes them when the execution emits a durable change event.
 */
const ActiveSubagentsPanel: React.FC<{ executionId: ExecutionId }> = ({ executionId }) => {
  const { t } = useTranslation();
  const [items, setItems] = useState<TActiveSubagent[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const requestSequence = useRef(0);
  const refreshTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const load = useCallback(async () => {
    const request = ++requestSequence.current;
    try {
      const next = await ipcBridge.agentExecution.listActiveSubagents.invoke({
        query: { execution_id: executionId, limit: LIMIT },
      });
      if (request !== requestSequence.current) return;
      setItems(next);
      setError(null);
    } catch (cause) {
      if (request !== requestSequence.current) return;
      setError(String(cause));
    } finally {
      if (request === requestSequence.current) setLoading(false);
    }
  }, [executionId]);

  useEffect(() => {
    setLoading(true);
    void load();
  }, [load]);

  useEffect(() => {
    const unsubscribe = ipcBridge.agentExecution.events.changed.on((event) => {
      if (event.execution_id !== executionId) return;
      if (refreshTimer.current !== null) clearTimeout(refreshTimer.current);
      refreshTimer.current = setTimeout(() => {
        refreshTimer.current = null;
        void load();
      }, REFRESH_DEBOUNCE_MS);
    });
    return () => {
      unsubscribe();
      requestSequence.current += 1;
      if (refreshTimer.current !== null) clearTimeout(refreshTimer.current);
      refreshTimer.current = null;
    };
  }, [executionId, load]);

  if (loading && items.length === 0) {
    return (
      <div className='px-12px py-6px text-11px text-t-tertiary' data-testid='active-subagents-loading'>
        {t('agentExecution.subagents.loading')}
      </div>
    );
  }

  if (error !== null) {
    return (
      <div className='px-12px py-6px text-11px text-danger-6' role='alert'>
        {t('agentExecution.subagents.loadError', { error })}
      </div>
    );
  }

  if (items.length === 0) return null;

  return (
    <section
      className='mx-10px mb-6px flex flex-col gap-6px rd-8px border border-solid border-[var(--border-base)] bg-fill-2 px-10px py-8px'
      aria-label={t('agentExecution.subagents.title')}
      data-testid='active-subagents-panel'
    >
      <header className='flex items-center justify-between gap-8px'>
        <span className='text-12px font-500 text-t-primary'>{t('agentExecution.subagents.title')}</span>
        <span className='text-11px text-t-tertiary'>
          {t('agentExecution.subagents.activeCount', { count: items.length })}
        </span>
      </header>
      <ul className='flex max-h-120px flex-col gap-2px overflow-auto'>
        {items.map((item) => (
          <li
            key={item.attempt_id}
            className='flex min-w-0 items-center gap-8px rd-6px px-4px py-3px text-11px'
          >
            <span
              className={`h-6px w-6px shrink-0 rd-full ${statusDotClass(item.attempt_status)}`}
              aria-hidden='true'
            />
            <span className='min-w-0 flex-1 truncate text-t-primary' title={item.step_title}>
              {item.step_title}
            </span>
            <span className='shrink-0 text-t-tertiary'>
              {item.participant_role ?? t('agentExecution.subagents.roleFallback')}
            </span>
            <span className='shrink-0 text-t-tertiary'>
              {t(`agentExecution.status.step.${item.step_status}`)}
            </span>
            {item.question !== null && (
              <span
                className='max-w-160px shrink-0 truncate text-warning-6'
                title={t('agentExecution.subagents.waitingQuestion', { question: item.question })}
              >
                {t('agentExecution.subagents.waitingQuestion', { question: item.question })}
              </span>
            )}
          </li>
        ))}
      </ul>
    </section>
  );
};

export default ActiveSubagentsPanel;
