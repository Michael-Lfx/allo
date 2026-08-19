/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useCallback, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, Modal, Message, Tooltip } from '@arco-design/web-react';
import { Copy, FullScreen, Info } from '@icon-park/react';
import { JsonView, defaultStyles } from 'react-json-view-lite';
import 'react-json-view-lite/dist/index.css';
import { copyText } from '@renderer/utils/ui/clipboard';
import { formatJson } from './format';

const treeStyle = {
  ...defaultStyles,
  container: `${defaultStyles.container} session-logs-json-tree__container`,
  label: 'session-logs-json-tree__label',
  clickableLabel: 'session-logs-json-tree__label session-logs-json-tree__label--click',
  nullValue: 'session-logs-json-tree__value',
  undefinedValue: 'session-logs-json-tree__value',
  stringValue: 'session-logs-json-tree__value',
  booleanValue: 'session-logs-json-tree__value',
  numberValue: 'session-logs-json-tree__value',
  otherValue: 'session-logs-json-tree__value',
  punctuation: 'session-logs-json-tree__punct',
  expandIcon: `${defaultStyles.expandIcon} session-logs-json-tree__expander`,
  collapseIcon: `${defaultStyles.collapseIcon} session-logs-json-tree__expander`,
  collapsedContent: `${defaultStyles.collapsedContent} session-logs-json-tree__collapsed`,
};

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

const TreeBody: React.FC<{ value: unknown; forceText?: boolean }> = ({ value, forceText }) => {
  const { t } = useTranslation();
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

export interface ObservationJsonTreeProps {
  label: string;
  value: unknown;
  hint?: string;
  /** Render strings and primitives as text even when wrapping objects exist. */
  textValue?: boolean;
}

const ObservationJsonTree: React.FC<ObservationJsonTreeProps> = ({
  label,
  value,
  hint,
  textValue = false,
}) => {
  const { t } = useTranslation();
  const [maximized, setMaximized] = useState(false);
  const copyPayload = useMemo(() => formatJson(value), [value]);
  const copyLabel = t('conversation.agentTrace.copyField', { label });

  const onCopy = useCallback(async () => {
    try {
      await copyText(copyPayload);
      Message.success(t('conversation.agentTrace.copied'));
    } catch {
      Message.error(t('conversation.agentTrace.copyFailed'));
    }
  }, [copyPayload, t]);

  return (
    <div className='session-logs-json-tree'>
      <div className='session-logs-json-tree__toolbar'>
        <div className='session-logs-json-tree__caption'>
          {label}
          {hint ? (
            <Tooltip content={hint}>
              <button type='button' className='session-logs-info' aria-label={hint}>
                <Info theme='outline' size='12' strokeWidth={3} />
              </button>
            </Tooltip>
          ) : null}
        </div>
        <div className='session-logs-json-tree__actions'>
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
      <div className='session-logs-json-tree__body'>
        <TreeBody value={value} forceText={textValue} />
      </div>
      <Modal
        title={label}
        visible={maximized}
        onCancel={() => setMaximized(false)}
        footer={null}
        unmountOnExit
        style={{ width: 'min(920px, 92vw)' }}
      >
        <div className='session-logs-json-tree session-logs-json-tree--modal'>
          <TreeBody value={value} forceText={textValue} />
        </div>
      </Modal>
    </div>
  );
};

export default ObservationJsonTree;
