/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, Message, Tag, Tooltip } from '@arco-design/web-react';
import { Copy } from '@icon-park/react';
import { copyText } from '@renderer/utils/ui/clipboard';
import { formatBytes, formatClock, shortId } from './format';
import type { AgentTraceArtifactIndexEntry, AgentTraceArtifactMeta } from './useAgentTraces';

export interface TraceSessionArtifactsProps {
  entries: AgentTraceArtifactIndexEntry[];
  loading?: boolean;
  selectedTraceId?: string | null;
  onSelectTrace?: (traceId: string) => void;
}

function ArtifactRowMeta({
  artifact,
  compact,
}: {
  artifact: AgentTraceArtifactMeta;
  compact?: boolean;
}) {
  const { t } = useTranslation();
  const onCopy = useCallback(
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

  return (
    <div className={`min-w-0 ${compact ? '' : 'flex flex-col gap-2px'}`}>
      <div className='flex items-center gap-6px min-w-0'>
        <Tag size='small' color='cyan'>
          {artifact.kind || 'file'}
        </Tag>
        {artifact.source === 'reported' ? (
          <Tag size='small' color='orangered'>
            {t('conversation.agentTrace.reported')}
          </Tag>
        ) : artifact.source === 'receipt' ? (
          <Tag size='small' color='green'>
            {t('conversation.agentTrace.receipt')}
          </Tag>
        ) : null}
        <span
          className='text-12px text-[var(--color-text-1)] font-mono truncate flex-1'
          title={artifact.relative_path}
        >
          {artifact.relative_path}
        </span>
        <Tooltip content={t('conversation.agentTrace.copyPath')}>
          <Button
            type='text'
            size='mini'
            className='shrink-0 !p-0 !h-18px !w-18px'
            icon={<Copy theme='outline' size='12' strokeWidth={3} />}
            onClick={(e) => {
              e.stopPropagation();
              void onCopy(artifact.relative_path);
            }}
            aria-label={t('conversation.agentTrace.copyPath')}
          />
        </Tooltip>
      </div>
      <div className='text-10px text-[var(--color-text-3)] flex flex-wrap gap-x-8px'>
        <span>{formatBytes(artifact.size_bytes)}</span>
        {artifact.mime_type ? <span>{artifact.mime_type}</span> : null}
        {artifact.tool_name ? <span>tool={artifact.tool_name}</span> : null}
        <span className='font-mono' title={artifact.id}>
          id={shortId(artifact.id)}
        </span>
      </div>
    </div>
  );
}

const TraceSessionArtifacts: React.FC<TraceSessionArtifactsProps> = ({
  entries,
  loading,
  selectedTraceId,
  onSelectTrace,
}) => {
  const { t } = useTranslation();

  return (
    <div className='border-b border-solid border-[var(--color-border-2)]'>
      <div className='px-12px pt-10px pb-6px flex items-center justify-between gap-8px'>
        <div className='text-11px font-600 text-[var(--color-text-2)]'>
          {t('conversation.agentTrace.sessionArtifacts')} · {entries.length}
        </div>
        <div className='text-10px text-[var(--color-text-3)]'>
          {t('conversation.agentTrace.sessionArtifactsHint')}
        </div>
      </div>
      {loading ? (
        <div className='px-12px pb-10px text-12px text-[var(--color-text-3)]'>…</div>
      ) : entries.length === 0 ? (
        <div className='px-12px pb-10px text-11px text-[var(--color-text-3)]'>
          {t('conversation.agentTrace.emptyArtifacts')}
        </div>
      ) : (
        <div className='max-h-160px overflow-auto px-8px pb-8px flex flex-col gap-4px'>
          {entries.map((row) => {
            const active = row.trace_id === selectedTraceId;
            return (
              <button
                key={`${row.trace_id}:${row.artifact.id}`}
                type='button'
                className='w-full text-left px-8px py-6px rounded-4px border border-solid cursor-pointer'
                style={{
                  borderColor: active ? 'var(--color-text-2)' : 'var(--color-border-2)',
                  background: active
                    ? 'color-mix(in srgb, var(--color-text-2) 8%, var(--color-bg-1))'
                    : 'var(--color-bg-1)',
                }}
                onClick={() => onSelectTrace?.(row.trace_id)}
              >
                <ArtifactRowMeta artifact={row.artifact} />
                <div className='text-10px text-[var(--color-text-3)] mt-3px flex gap-x-8px flex-wrap'>
                  <span>{formatClock(row.started_at_ms)}</span>
                  <span className='font-mono' title={row.msg_id}>
                    msg={shortId(row.msg_id)}
                  </span>
                  <span className='font-mono' title={row.trace_id}>
                    trace={shortId(row.trace_id)}
                  </span>
                </div>
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
};

export { ArtifactRowMeta };
export default TraceSessionArtifacts;
