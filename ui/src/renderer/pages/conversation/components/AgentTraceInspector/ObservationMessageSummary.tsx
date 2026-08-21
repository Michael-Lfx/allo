/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React from 'react';
import { useTranslation } from 'react-i18next';
import { Tooltip } from '@arco-design/web-react';
import {
  joinOmittedMark,
  type MessagePreview,
  type MessageScanRow,
} from './observationScan';

type Translate = (key: string, options?: Record<string, unknown>) => string;

const KNOWN_ROLES = new Set(['user', 'assistant', 'system', 'tool']);

function kindLabel(kind: string, t: Translate): string {
  if (kind === 'text') return t('conversation.agentTrace.partText');
  if (kind === 'tool_use') return t('conversation.agentTrace.scanKindToolUse');
  if (kind === 'tool_result') return t('conversation.agentTrace.scanToolResult');
  if (kind === 'thinking') return t('conversation.agentTrace.scanThinking');
  if (kind === 'image') return t('conversation.agentTrace.scanImage');
  return kind;
}

function previewBody(preview: MessagePreview, t: Translate): string {
  if (preview.kind === 'text') return preview.text;
  if (preview.kind === 'tool_use') {
    return t('conversation.agentTrace.scanToolUse', { name: preview.name });
  }
  if (preview.kind === 'tool_result') {
    if (preview.isError) {
      return preview.text
        ? `${t('conversation.agentTrace.scanToolResultError')} · ${preview.text}`
        : t('conversation.agentTrace.scanToolResultError');
    }
    return preview.text || t('conversation.agentTrace.scanToolResult');
  }
  if (preview.kind === 'thinking') {
    return preview.text || t('conversation.agentTrace.scanThinking');
  }
  if (preview.kind === 'image') {
    return preview.mediaType
      ? `${t('conversation.agentTrace.scanImage')} ${preview.mediaType}`
      : t('conversation.agentTrace.scanImage');
  }
  return '';
}

function formatMessagePreview(row: MessageScanRow, t: Translate): string {
  const body = previewBody(row.preview, t);
  if (row.kinds.length <= 1) return body;
  const labels = row.kinds.map((kind) => kindLabel(kind, t)).filter(Boolean);
  return [...labels, body].filter(Boolean).join(' · ');
}

function roleLabel(role: string, t: Translate): string {
  if (KNOWN_ROLES.has(role)) return t(`conversation.agentTrace.role_${role}`);
  return role;
}

export interface MessageScanPresentation {
  visibleText: string;
  tooltipText?: string;
  tipFallback: boolean;
}

/**
 * Build the visible and hover-only text for one message row. Keeping this
 * projection separate from the DOM makes the Context visibility contract
 * testable without relying on a browser tooltip implementation.
 */
export function messageScanPresentation(
  row: MessageScanRow,
  t: Translate,
): MessageScanPresentation {
  const primaryPreview = joinOmittedMark(
    formatMessagePreview(row, t),
    row.omittedReason,
    t('conversation.agentTrace.omittedField'),
  );
  const visibleText = primaryPreview || (row.context ? '' : t('conversation.agentTrace.previewMissing'));
  const tooltipText = row.context
    ? [
        t('conversation.agentTrace.scanContext'),
        joinOmittedMark(
          row.context.text,
          row.context.omittedReason,
          t('conversation.agentTrace.omittedField'),
        ),
      ]
        .filter(Boolean)
        .join(' · ')
    : undefined;
  return {
    visibleText,
    tooltipText,
    tipFallback: !row.context || Boolean(primaryPreview),
  };
}

export const ScanTip: React.FC<{
  content: string;
  className: string;
  children: React.ReactNode;
}> = ({ content, className, children }) => {
  const trimmed = content.trim();
  if (!trimmed) {
    return <span className={className}>{children}</span>;
  }
  return (
    <Tooltip
      content={<div className='session-logs-scan__tip'>{trimmed}</div>}
      position='top'
      getPopupContainer={() => document.body}
    >
      <span className={className}>{children}</span>
    </Tooltip>
  );
};

export const ScanPreview: React.FC<{
  text: string;
  tip?: string;
  tipFallback?: boolean;
}> = ({ text, tip, tipFallback = true }) => {
  const content = (tip ?? text).trim();
  const shown = text.trim();
  if (!shown && !content) {
    return <span className='session-logs-scan__preview' />;
  }
  return (
    <ScanTip content={content} className='session-logs-scan__preview-wrap'>
      <span className='session-logs-scan__preview'>{shown || (tipFallback ? content : '')}</span>
    </ScanTip>
  );
};

export const MessageScanList: React.FC<{ rows: MessageScanRow[]; newestFirst: boolean }> = ({
  rows,
  newestFirst,
}) => {
  const { t } = useTranslation();
  const ordered = newestFirst ? [...rows].reverse() : rows;
  return (
    <ol className='session-logs-scan'>
      {ordered.map((row) => {
        const presentation = messageScanPresentation(row, t);
        return (
          <li key={row.index} className='session-logs-scan__row'>
            <span className='session-logs-scan__role'>{roleLabel(row.role, t) || '-'}</span>
            <ScanPreview
              text={presentation.visibleText}
              tip={presentation.tooltipText}
              tipFallback={presentation.tipFallback}
            />
          </li>
        );
      })}
    </ol>
  );
};
