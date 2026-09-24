/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { Tag, Typography } from '@arco-design/web-react';
import { useTranslation } from 'react-i18next';
import type { TFunction } from 'i18next';
import type { EvalCaseTraceView } from './api';
import { eventKindColor } from './format';

const { Text } = Typography;

function eventKindLabel(kind: string, t: TFunction): string {
  switch (kind) {
    case 'text':
      return t('eval.trace.kind.text');
    case 'thinking':
      return t('eval.trace.kind.thinking');
    case 'tool_call':
      return t('eval.trace.kind.tool_call');
    case 'tool_result':
      return t('eval.trace.kind.tool_result');
    case 'error':
      return t('eval.trace.kind.error');
    case 'info':
      return t('eval.trace.kind.info');
    default:
      return kind;
  }
}

export function TraceView({ trace }: { trace: EvalCaseTraceView }) {
  const { t } = useTranslation();
  return (
    <div className='flex flex-col gap-12px'>
      <Text className='font-500'>
        {t('eval.trace.title')}
        {trace.live ? ` · ${t('eval.trace.live')}` : ''}
      </Text>
      {trace.events.length === 0 && !trace.assistant_text ? (
        <Text type='secondary'>{t('eval.trace.empty')}</Text>
      ) : (
        <ol className='m-0 flex list-none flex-col gap-8px p-0'>
          {trace.events.map((event, index) => (
            <li
              key={`${event.ts_ms}-${index}`}
              className='border-l-solid border-l-2px border-l-[var(--color-border-3)] pl-12px min-w-0'
            >
              <div className='flex flex-wrap items-center gap-8px'>
                <Tag size='small' color={eventKindColor(event.kind, event.is_error)}>
                  {eventKindLabel(event.kind, t)}
                </Tag>
                {event.name && (
                  <Text className='truncate' translate='no'>
                    {event.name}
                  </Text>
                )}
                {event.is_error ? (
                  <Tag size='small' color='red'>
                    {t('eval.fail')}
                  </Tag>
                ) : null}
              </div>
              {event.input && (
                <pre className='m-0 mt-4px max-h-240px overflow-auto whitespace-pre-wrap break-words text-12px text-t-secondary'>
                  {event.input}
                </pre>
              )}
              {event.content && (
                <pre className='m-0 mt-4px max-h-240px overflow-auto whitespace-pre-wrap break-words text-12px text-t-secondary'>
                  {event.content}
                </pre>
              )}
            </li>
          ))}
        </ol>
      )}
      {trace.assistant_text && (
        <div>
          <Text type='secondary'>{t('eval.trace.assistant')}</Text>
          <pre className='m-0 mt-4px max-h-320px overflow-auto whitespace-pre-wrap break-words text-12px'>
            {trace.assistant_text}
          </pre>
        </div>
      )}
      <div>
        <Text type='secondary'>{t('eval.trace.artifacts')}</Text>
        {trace.artifacts.length === 0 ? (
          <Text type='secondary' className='block'>
            {t('eval.trace.noArtifacts')}
          </Text>
        ) : (
          <ul className='m-0 mt-8px flex list-none flex-col gap-12px p-0'>
            {trace.artifacts.map((artifact) => (
              <li key={artifact.path} className='min-w-0'>
                <Text className='break-all' translate='no'>
                  {artifact.path}
                  <Text type='secondary' className='ml-8px'>
                    {artifact.kind === 'binary'
                      ? t('eval.trace.binary')
                      : t('eval.trace.size', { bytes: artifact.size_bytes })}
                  </Text>
                </Text>
                {artifact.preview && (
                  <pre className='m-0 mt-4px max-h-240px overflow-auto whitespace-pre-wrap break-words text-12px'>
                    {artifact.preview}
                  </pre>
                )}
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
