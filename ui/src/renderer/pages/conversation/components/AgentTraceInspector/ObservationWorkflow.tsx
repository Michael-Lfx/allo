/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, Collapse, Message, Tag, Tooltip } from '@arco-design/web-react';
import { Copy } from '@icon-park/react';
import { copyText } from '@renderer/utils/ui/clipboard';
import { formatJson, shortId } from './format';
import {
  asRecord,
  canonicalRequestFromPayload,
  toolStatus,
  type ProjectedGap,
  type ProjectedModelCall,
  type ProjectedToolExecution,
  type ProjectedTurn,
} from './useAgentTraces';

export interface ObservationWorkflowProps {
  turn: ProjectedTurn;
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
    <div>
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

const ToolBlock: React.FC<{ tool: ProjectedToolExecution }> = ({ tool }) => {
  const { t } = useTranslation();
  const status = toolStatus(tool);
  const payload =
    tool.cancelled ?? tool.failed ?? tool.completed ?? tool.started ?? {};
  return (
    <div className='rounded-4px border border-solid border-[var(--color-border-2)] px-8px py-8px'>
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

const ModelCallBlock: React.FC<{ call: ProjectedModelCall; index: number }> = ({
  call,
  index,
}) => {
  const { t } = useTranslation();
  const requestBody = canonicalRequestFromPayload(call.request);
  const responseRecord = asRecord(call.response);

  return (
    <div className='flex flex-col gap-8px px-12px py-10px border-b border-solid border-[var(--color-border-2)]'>
      <div className='flex items-center gap-6px flex-wrap'>
        <span className='text-10px text-[var(--color-text-3)] tabular-nums'>{index + 1}</span>
        <Tag size='small' color='arcoblue'>
          {call.call_kind ?? t('conversation.agentTrace.callKind')}
        </Tag>
        {call.observation_scope ? (
          <Tag size='small' color='gray'>
            {call.observation_scope}
          </Tag>
        ) : null}
        {call.interrupted ? (
          <Tag size='small' color='orangered'>
            {t('conversation.agentTrace.interrupted')}
          </Tag>
        ) : null}
        <span className='text-11px font-mono text-[var(--color-text-3)]' title={call.model_call_id}>
          {shortId(call.model_call_id)}
        </span>
      </div>

      {call.request != null ? (
        <JsonBlock label={t('conversation.agentTrace.request')} value={requestBody} />
      ) : (
        <div className='text-12px text-[var(--color-text-3)]'>
          {t('conversation.agentTrace.request')} · —
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
        <div className='rounded-4px bg-[var(--color-fill-1)] px-8px py-8px text-12px text-[var(--color-text-2)]'>
          {t('conversation.agentTrace.noResponse')}
        </div>
      )}

      {call.tools.length > 0 ? (
        <div className='flex flex-col gap-6px'>
          <div className='text-11px font-600 text-[var(--color-text-2)]'>
            {t('conversation.agentTrace.tools')} · {call.tools.length}
          </div>
          {call.tools.map((tool) => (
            <ToolBlock key={tool.tool_call_id || formatJson(tool)} tool={tool} />
          ))}
        </div>
      ) : null}
    </div>
  );
};

const ObservationWorkflow: React.FC<ObservationWorkflowProps> = ({ turn }) => {
  const { t } = useTranslation();

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
    <div>
      {(degraded || turn.interrupted || turn.gap_count > 0) && (
        <div
          className='mx-12px mt-10px mb-2px rounded-4px border border-solid px-10px py-8px'
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

      <div className='px-12px pt-10px pb-6px flex items-center justify-between gap-8px flex-wrap'>
        <div className='flex items-center gap-8px min-w-0 flex-wrap'>
          <Tag size='small' color={degraded ? 'orangered' : 'green'}>
            {degraded
              ? t('conversation.agentTrace.integrityDegraded')
              : t('conversation.agentTrace.integrityComplete')}
          </Tag>
          {turn.session_kind ? <Tag size='small'>{turn.session_kind}</Tag> : null}
        </div>
        <Button type='outline' size='mini' onClick={() => void copyJson()}>
          {t('conversation.agentTrace.copyJson')}
        </Button>
      </div>

      <div className='px-12px pb-10px'>
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
        <div className='px-12px pb-8px flex flex-col gap-6px'>
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
        turn.model_calls.map((call, index) => (
          <ModelCallBlock key={call.model_call_id || `call-${index}`} call={call} index={index} />
        ))
      )}
    </div>
  );
};

export default ObservationWorkflow;
