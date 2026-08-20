/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, Modal, Message, Tooltip } from '@arco-design/web-react';
import { Copy, FullScreen, Hourglass, Info, SortAmountDown, SortAmountUp } from '@icon-park/react';
import { JsonView, defaultStyles } from 'react-json-view-lite';
import 'react-json-view-lite/dist/index.css';
import { copyText } from '@renderer/utils/ui/clipboard';
import { formatJson } from './format';
import {
  joinOmittedMark,
  projectObservationScan,
  type MessagePreview,
  type MessageScanRow,
  type ObservationScanKind,
  type ObservationScanResult,
  type ToolDefScanRow,
} from './observationScan';

const treeStyle = {
  ...defaultStyles,
  container: 'session-logs-json-tree__container',
  label: 'session-logs-json-tree__label',
  clickableLabel: 'session-logs-json-tree__label session-logs-json-tree__label--click',
  nullValue: 'session-logs-json-tree__value',
  undefinedValue: 'session-logs-json-tree__value',
  stringValue: 'session-logs-json-tree__value',
  booleanValue: 'session-logs-json-tree__value',
  numberValue: 'session-logs-json-tree__value',
  otherValue: 'session-logs-json-tree__value',
  punctuation: 'session-logs-json-tree__punct',
  expandIcon: 'session-logs-json-tree__expander session-logs-json-tree__expander--closed',
  collapseIcon: 'session-logs-json-tree__expander session-logs-json-tree__expander--open',
  collapsedContent: 'session-logs-json-tree__collapsed',
};

type Translate = (key: string, options?: Record<string, unknown>) => string;

const KNOWN_ROLES = new Set(['user', 'assistant', 'system', 'tool']);

/** Root and `messages`/`tools` arrays expand; array elements stay collapsed. */
export function shouldExpandObservationNode(
  level: number,
  value: unknown,
  field?: string
): boolean {
  if (level === 0) return true;
  if ((field === 'messages' || field === 'tools') && Array.isArray(value)) return true;
  return false;
}

function forwardPunctuationToggle(event: React.MouseEvent<HTMLElement>) {
  const target = event.target;
  if (!(target instanceof Element)) return;
  if (target.closest('[role="button"]')) return;
  if (!target.closest('.session-logs-json-tree__punct')) return;
  const item = target.closest('[role="treeitem"]');
  if (!(item instanceof HTMLElement)) return;
  item.querySelector<HTMLElement>(':scope > [role="button"]')?.click();
}

function isTreeData(value: unknown): value is Record<string, unknown> | unknown[] {
  return value != null && typeof value === 'object';
}

function asDisplayText(value: unknown): string | null {
  if (value == null) return null;
  if (typeof value === 'string') return value;
  if (typeof value === 'number' || typeof value === 'boolean' || typeof value === 'bigint') {
    return String(value);
  }
  return null;
}

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

const TreeBody: React.FC<{ value: unknown; forceText?: boolean }> = ({ value, forceText }) => {
  const { t } = useTranslation();
  if (value == null) {
    return (
      <pre className='session-logs-json-tree__text'>
        {t('conversation.agentTrace.previewMissing')}
      </pre>
    );
  }
  const text = asDisplayText(value);
  if (forceText || text != null) {
    return (
      <pre className='session-logs-json-tree__text'>
        {text ?? t('conversation.agentTrace.previewMissing')}
      </pre>
    );
  }
  if (!isTreeData(value)) {
    return <pre className='session-logs-json-tree__text'>{formatJson(value)}</pre>;
  }
  return (
    <div className='session-logs-json-tree__hit' onClick={forwardPunctuationToggle}>
      <JsonView
        data={value}
        style={treeStyle}
        shouldExpandNode={shouldExpandObservationNode}
        clickToExpandNode
      />
    </div>
  );
};

const OmittedScan: React.FC<{ reason: string }> = ({ reason }) => {
  const { t } = useTranslation();
  return (
    <div className='session-logs-scan__omitted'>
      <div className='session-logs-scan__omitted-title'>
        {t('conversation.agentTrace.omittedField')}
      </div>
      <div>
        {t('conversation.agentTrace.omittedReason')}: {reason}
      </div>
    </div>
  );
};

function firstLine(text: string): string {
  return (text.split(/\r?\n/, 1)[0] ?? '').trim();
}

function hintLines(hint: string): string[] {
  return hint.split('\n').map((line) => line.trim()).filter(Boolean);
}

const HintTip: React.FC<{ hint: string }> = ({ hint }) => (
  <div className='session-logs-scan__tip'>
    {hintLines(hint).map((line) => (
      <div key={line} className='session-logs-scan__tip-line'>
        {line}
      </div>
    ))}
  </div>
);

const ScanTip: React.FC<{
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

const ScanPreview: React.FC<{ text: string; tip?: string }> = ({ text, tip }) => {
  const content = (tip ?? text).trim();
  const shown = text.trim();
  if (!shown && !content) {
    return <span className='session-logs-scan__preview' />;
  }
  return (
    <ScanTip content={content} className='session-logs-scan__preview-wrap'>
      <span className='session-logs-scan__preview'>{shown || content}</span>
    </ScanTip>
  );
};

const MessageScanList: React.FC<{ rows: MessageScanRow[]; newestFirst: boolean }> = ({
  rows,
  newestFirst,
}) => {
  const { t } = useTranslation();
  const ordered = newestFirst ? [...rows].reverse() : rows;
  return (
    <ol className='session-logs-scan'>
      {ordered.map((row) => {
        const preview =
          joinOmittedMark(
            formatMessagePreview(row, t),
            row.omittedReason,
            t('conversation.agentTrace.omittedField')
          ) || t('conversation.agentTrace.previewMissing');
        return (
          <li key={row.index} className='session-logs-scan__row'>
            <span className='session-logs-scan__role'>{roleLabel(row.role, t) || '-'}</span>
            <ScanPreview text={preview} />
          </li>
        );
      })}
    </ol>
  );
};

const ToolScanList: React.FC<{ rows: ToolDefScanRow[] }> = ({ rows }) => {
  const { t } = useTranslation();
  return (
    <ol className='session-logs-scan'>
      {rows.map((row) => {
        const name = row.name || t('conversation.agentTrace.previewMissing');
        return (
          <li key={row.index} className='session-logs-scan__row'>
            <span className='session-logs-scan__lead'>
              <ScanTip content={row.name} className='session-logs-scan__hover'>
                <span className='session-logs-scan__name'>{name}</span>
              </ScanTip>
              {row.deferred ? (
                <span
                  className='session-logs-scan__flag'
                  role='img'
                  aria-label={t('conversation.agentTrace.toolDeferred')}
                >
                  <Hourglass theme='outline' size='12' strokeWidth={3} />
                </span>
              ) : null}
            </span>
            <ScanPreview
              text={joinOmittedMark(
                firstLine(row.description),
                row.omittedReason,
                t('conversation.agentTrace.omittedField')
              )}
              tip={joinOmittedMark(
                row.description,
                row.omittedReason,
                t('conversation.agentTrace.omittedField')
              )}
            />
          </li>
        );
      })}
    </ol>
  );
};

const EmptyScan: React.FC<{ labelKey: 'scanEmptyMessages' | 'scanEmptyTools' }> = ({
  labelKey,
}) => {
  const { t } = useTranslation();
  return <div className='session-logs-scan__empty'>{t(`conversation.agentTrace.${labelKey}`)}</div>;
};

const ScanBody: React.FC<{ result: ObservationScanResult; newestFirst: boolean }> = ({
  result,
  newestFirst,
}) => {
  if (result.kind === 'omitted') return <OmittedScan reason={result.reason} />;
  if (result.kind === 'messages') {
    if (result.rows.length === 0) return <EmptyScan labelKey='scanEmptyMessages' />;
    return <MessageScanList rows={result.rows} newestFirst={newestFirst} />;
  }
  if (result.kind === 'tools') {
    if (result.rows.length === 0) return <EmptyScan labelKey='scanEmptyTools' />;
    return <ToolScanList rows={result.rows} />;
  }
  return null;
};

export interface ObservationJsonTreeProps {
  label: string;
  value: unknown;
  hint?: string;
  /** Render strings and primitives as text even when wrapping objects exist. */
  textValue?: boolean;
  scan?: ObservationScanKind;
  /** Call/tile identity. Poll updates must not pass a new key. */
  resetKey?: string;
}

const ObservationJsonTree: React.FC<ObservationJsonTreeProps> = ({
  label,
  value,
  hint,
  textValue = false,
  scan,
  resetKey = '',
}) => {
  const { t } = useTranslation();
  const [maximized, setMaximized] = useState(false);
  const [showRaw, setShowRaw] = useState(false);
  const [newestFirst, setNewestFirst] = useState(true);
  const copyPayload = useMemo(() => formatJson(value), [value]);
  const copyLabel = t('conversation.agentTrace.copyField', { label });
  const scanResult = useMemo(
    () => (scan ? projectObservationScan(value, scan) : { kind: 'unscannable' as const }),
    [scan, value]
  );
  const canScan = scan != null && scanResult.kind !== 'unscannable';

  useEffect(() => {
    setShowRaw(false);
    setNewestFirst(true);
  }, [resetKey]);

  const onCopy = useCallback(async () => {
    try {
      await copyText(copyPayload);
      Message.success(t('conversation.agentTrace.copied'));
    } catch {
      Message.error(t('conversation.agentTrace.copyFailed'));
    }
  }, [copyPayload, t]);

  const canSortMessages = canScan && scan === 'messages' && !showRaw;
  const sortLabel = newestFirst
    ? t('conversation.agentTrace.newestFirst')
    : t('conversation.agentTrace.oldestFirst');

  const renderBody = () =>
    canScan && !showRaw ? (
      <ScanBody result={scanResult} newestFirst={newestFirst} />
    ) : (
      <TreeBody value={value} forceText={textValue} />
    );

  return (
    <div className='session-logs-json-tree'>
      <div className='session-logs-json-tree__toolbar'>
        <div className='session-logs-json-tree__caption'>
          {label}
          {hint ? (
            <Tooltip
              content={<HintTip hint={hint} />}
              getPopupContainer={() => document.body}
            >
              <button
                type='button'
                className='session-logs-info'
                aria-label={hintLines(hint).join(' ')}
              >
                <Info theme='outline' size='12' strokeWidth={3} />
              </button>
            </Tooltip>
          ) : null}
        </div>
        <div className='session-logs-json-tree__actions'>
          {canScan ? (
            <button
              type='button'
              className='session-logs-scan__mode'
              aria-pressed={showRaw}
              onClick={() => setShowRaw((open) => !open)}
            >
              {showRaw
                ? t('conversation.agentTrace.inspectShowScan')
                : t('conversation.agentTrace.inspectShowRaw')}
            </button>
          ) : null}
          {canSortMessages ? (
            <Tooltip content={sortLabel}>
              <Button
                type='text'
                size='mini'
                className='session-logs-json-tree__icon-btn'
                icon={
                  newestFirst ? (
                    <SortAmountDown theme='outline' size='12' strokeWidth={3} />
                  ) : (
                    <SortAmountUp theme='outline' size='12' strokeWidth={3} />
                  )
                }
                aria-pressed={newestFirst}
                aria-label={sortLabel}
                onClick={() => setNewestFirst((value) => !value)}
              />
            </Tooltip>
          ) : null}
          <Tooltip content={copyLabel}>
            <Button
              type='text'
              size='mini'
              className='session-logs-json-tree__icon-btn'
              icon={<Copy theme='outline' size='12' strokeWidth={3} />}
              onClick={() => void onCopy()}
              aria-label={copyLabel}
            />
          </Tooltip>
          <Tooltip content={t('conversation.agentTrace.maximizeInspector')}>
            <Button
              type='text'
              size='mini'
              className='session-logs-json-tree__icon-btn'
              icon={<FullScreen theme='outline' size='12' strokeWidth={3} />}
              onClick={() => setMaximized(true)}
              aria-label={t('conversation.agentTrace.maximizeInspector')}
            />
          </Tooltip>
        </div>
      </div>
      <div className='session-logs-json-tree__body'>{renderBody()}</div>
      <Modal
        title={label}
        visible={maximized}
        onCancel={() => setMaximized(false)}
        footer={null}
        unmountOnExit
        style={{ width: 'min(920px, 92vw)' }}
      >
        <div className='session-logs-json-tree session-logs-json-tree--modal'>{renderBody()}</div>
      </Modal>
    </div>
  );
};

export default ObservationJsonTree;
