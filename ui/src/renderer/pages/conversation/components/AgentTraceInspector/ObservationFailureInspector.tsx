/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useCallback, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, Tooltip } from '@arco-design/web-react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { Attention, Copy, Down, FileCode, Right } from '@icon-park/react';
import { copyText } from '@renderer/utils/ui/clipboard';
import ObservationJsonTree from './ObservationJsonTree';
import type { ProjectedTurn } from './useAgentTraces';

export interface ObservationFailureInspectorProps {
  turn: ProjectedTurn;
}

export function buildReproBundle(turn: ProjectedTurn): Record<string, unknown> {
  return {
    schema_version: 1,
    generated_at_ms: Date.now(),
    environment: {
      platform:
        (typeof window !== 'undefined' && window.__os) ||
        (typeof navigator !== 'undefined' ? navigator.platform : 'unknown'),
      userAgent: typeof navigator !== 'undefined' ? navigator.userAgent : 'unknown',
    },
    turn: {
      root_turn_id: turn.root_turn_id,
      conversation_id: turn.conversation_id ?? null,
      msg_id: turn.msg_id ?? null,
      session_kind: turn.session_kind ?? null,
      execution_id: turn.execution_id ?? null,
      step_id: turn.step_id ?? null,
      execution_attempt_id: turn.execution_attempt_id ?? null,
      status: turn.status,
      integrity: turn.integrity,
      interrupted: turn.interrupted,
      error: turn.error ?? null,
      started_at_ms: turn.started_at_ms ?? null,
      ended_at_ms: turn.ended_at_ms ?? null,
      elapsed_ms: turn.elapsed_ms ?? null,
      prompt_preview: turn.prompt_preview ?? null,
      prompt_preview_context_only: turn.prompt_preview_context_only ?? false,
      max_event_seq: turn.max_event_seq,
      has_turn_start: turn.has_turn_start,
      has_turn_end: turn.has_turn_end,
      gap_count: turn.gap_count,
      timeline: turn.timeline,
      model_calls: turn.model_calls,
      gaps: turn.gaps,
    },
  };
}

export const ObservationFailureInspector: React.FC<ObservationFailureInspectorProps> = ({ turn }) => {
  const { t } = useTranslation();
  const [rawOpen, setRawOpen] = useState(false);

  const errorGaps = useMemo(
    () => turn.gaps.filter((gap) => Boolean(gap.error && gap.error.trim().length > 0)),
    [turn.gaps]
  );

  const trimmedError = turn.error?.trim() || null;
  const hasTurnError = Boolean(trimmedError);
  const isFailed = turn.status === 'failed';
  const isInterruptedWithError = turn.interrupted && hasTurnError;
  const hasGapError = errorGaps.length > 0;

  const shouldRender = isFailed || hasTurnError || isInterruptedWithError || hasGapError;

  const onCopyError = useCallback(
    async (text: string) => {
      try {
        await copyText(text);
        Message.success(t('conversation.agentTrace.copied'));
      } catch {
        Message.error(t('conversation.agentTrace.copyFailed'));
      }
    },
    [t]
  );

  const onCopyRepro = useCallback(async () => {
    try {
      const bundle = buildReproBundle(turn);
      await copyText(JSON.stringify(bundle, null, 2));
      Message.success(t('conversation.agentTrace.failureCopiedRepro'));
    } catch {
      Message.error(t('conversation.agentTrace.copyFailed'));
    }
  }, [turn, t]);

  if (!shouldRender) {
    return null;
  }

  const rawPayload = {
    root_turn_id: turn.root_turn_id,
    status: turn.status,
    integrity: turn.integrity,
    interrupted: turn.interrupted,
    error: turn.error ?? null,
    gap_count: turn.gap_count,
    gaps: turn.gaps,
  };

  return (
    <div
      className='session-logs-failure-inspector'
      role='region'
      aria-label={t('conversation.agentTrace.failureInspectorTitle')}
    >
      <div className='session-logs-failure-inspector__header'>
        <div className='flex items-center gap-8px min-w-0'>
          <Attention theme='filled' size='16' fill='rgb(var(--danger-6))' className='shrink-0' />
          <span className='font-semibold text-13px text-[var(--color-text-1)] truncate'>
            {t('conversation.agentTrace.failureInspectorTitle')}
          </span>
          <span className='session-logs-failure-inspector__badge shrink-0'>
            {t('conversation.agentTrace.status_' + (turn.status ?? 'failed'))}
          </span>
        </div>
        <div className='flex items-center gap-6px shrink-0'>
          <Tooltip content={t('conversation.agentTrace.failureCopyRepro')}>
            <Button
              type='text'
              size='mini'
              className='!p-0 !h-20px !w-20px text-[var(--color-text-2)]'
              icon={<FileCode theme='outline' size='13' strokeWidth={3} />}
              onClick={() => void onCopyRepro()}
              aria-label={t('conversation.agentTrace.failureCopyRepro')}
            />
          </Tooltip>
          {trimmedError ? (
            <Tooltip content={t('conversation.agentTrace.failureCopyError')}>
              <Button
                type='text'
                size='mini'
                className='!p-0 !h-20px !w-20px text-[var(--color-text-2)]'
                icon={<Copy theme='outline' size='13' strokeWidth={3} />}
                onClick={() => void onCopyError(trimmedError)}
                aria-label={t('conversation.agentTrace.failureCopyError')}
              />
            </Tooltip>
          ) : null}
        </div>
      </div>

      <div className='session-logs-failure-inspector__body'>
        <p className='text-11px text-[var(--color-text-2)] m-0'>
          {t('conversation.agentTrace.failureInspectorDesc')}
        </p>

        {turn.model_calls.length === 0 && (isFailed || hasTurnError) ? (
          <div className='session-logs-failure-inspector__prep-callout'>
            <div className='session-logs-failure-inspector__prep-title'>
              {t('conversation.agentTrace.failurePreparationPhase')}
            </div>
            <div className='text-11px text-[var(--color-text-2)] mt-2px'>
              {t('conversation.agentTrace.failurePreparationHint')}
            </div>
          </div>
        ) : null}

        {trimmedError ? (
          <div className='session-logs-failure-inspector__error-box'>
            <div className='text-10px uppercase tracking-wider text-[var(--color-text-3)] mb-4px font-semibold'>
              {t('conversation.agentTrace.failureError')}
            </div>
            <pre className='session-logs-failure-inspector__pre'>{trimmedError}</pre>
          </div>
        ) : null}

        {hasGapError ? (
          <div className='flex flex-col gap-6px'>
            <div className='text-10px uppercase tracking-wider text-[var(--color-text-3)] font-semibold'>
              {t('conversation.agentTrace.failureGapHeader')}
            </div>
            {errorGaps.map((gap) => (
              <div key={gap.event_seq} className='session-logs-failure-inspector__gap-row'>
                <div className='flex items-center gap-6px'>
                  <span className='session-logs-failure-inspector__gap-seq'>
                    {t('conversation.agentTrace.gapSeq', { seq: gap.event_seq })}
                  </span>
                  {gap.reason ? (
                    <span className='text-11px text-[var(--color-text-3)]'>({gap.reason})</span>
                  ) : null}
                </div>
                <div className='session-logs-failure-inspector__gap-error'>
                  {gap.error}
                </div>
              </div>
            ))}
          </div>
        ) : null}

        <div className='mt-2px'>
          <button
            type='button'
            className='session-logs-identity-toggle'
            aria-expanded={rawOpen}
            onClick={() => setRawOpen((v) => !v)}
          >
            {t('conversation.agentTrace.failureRawDetails')}
            <span className='session-logs-identity-toggle__chevron'>
              {rawOpen ? (
                <Down theme='outline' size='12' strokeWidth={3} />
              ) : (
                <Right theme='outline' size='12' strokeWidth={3} />
              )}
            </span>
          </button>
          {rawOpen ? (
            <div className='mt-6px'>
              <ObservationJsonTree
                label={t('conversation.agentTrace.failureRawDetails')}
                value={rawPayload}
                collapsible={false}
              />
            </div>
          ) : null}
        </div>
      </div>
    </div>
  );
};

export default ObservationFailureInspector;
