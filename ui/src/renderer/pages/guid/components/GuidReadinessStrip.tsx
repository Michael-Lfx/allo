/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { TProviderWithModel } from '@/common/config/storage';
import { CheckOne, SettingConfig } from '@icon-park/react';
import React from 'react';
import { useTranslation } from 'react-i18next';
import styles from '../index.module.css';
import type { GuidReadinessResult, GuidTaskReceipt } from '../readiness/guidReadiness';

type GuidReadinessStripProps = {
  agentLabel: string;
  model?: TProviderWithModel | null;
  workspaceDir?: string;
  readiness: GuidReadinessResult;
  receipt: GuidTaskReceipt | null;
  onOpenSettings: () => void;
  onAddModel: () => void;
  onLinkWorkspace: () => void;
};

const GuidReadinessStrip: React.FC<GuidReadinessStripProps> = ({
  agentLabel,
  model,
  workspaceDir,
  readiness,
  receipt,
  onOpenSettings,
  onAddModel,
  onLinkWorkspace,
}) => {
  const { t } = useTranslation();
  const modelLabel = model?.use_model || model?.name;

  const handleClick = () => {
    switch (readiness.primaryAction) {
      case 'addModel':
        onAddModel();
        return;
      case 'linkWorkspace':
        onLinkWorkspace();
        return;
      case 'send':
        onOpenSettings();
        return;
      default: {
        const _exhaustive: never = readiness.primaryAction;
        void _exhaustive;
        onOpenSettings();
      }
    }
  };

  const primaryText = (() => {
    switch (readiness.blocker) {
      case 'model':
        return t('guid.readiness.needsModelCta', {
          agent: agentLabel,
          defaultValue: '{{agent}} 需要模型 · 点此原位连接',
        });
      case 'workspace':
        return t('guid.readiness.needsWorkspaceCta', {
          defaultValue: '此任务需要项目 · 点此选择文件夹',
        });
      case null:
        return modelLabel
          ? t('guid.readiness.ready', { agent: agentLabel, model: modelLabel })
          : t('guid.readiness.readyNoModel', {
              agent: agentLabel,
              defaultValue: '{{agent}} 已就绪 · 可以发送',
            });
      default: {
        const _exhaustive: never = readiness.blocker;
        return _exhaustive;
      }
    }
  })();

  const metaText = (() => {
    if (receipt) {
      return t(receipt.expectedArtifactKey, { defaultValue: receipt.expectedArtifactDefault });
    }
    if (workspaceDir) return t('guid.readiness.workspaceLinked');
    if (readiness.requiresWorkspace) return t('guid.readiness.workspaceRequired');
    return t('guid.readiness.workspaceOptional');
  })();

  return (
    <div className={styles.guidReadinessBlock} data-testid='guid-readiness-block'>
      {receipt ? (
        <div className={styles.guidTaskReceipt} data-testid='guid-task-receipt'>
          <div className={styles.guidTaskReceiptRow}>
            <span className={styles.guidTaskReceiptLabel}>
              {t('guid.taskReceipt.goal', { defaultValue: '目标' })}
            </span>
            <span className={styles.guidTaskReceiptValue}>{receipt.goal}</span>
          </div>
          <div className={styles.guidTaskReceiptRow}>
            <span className={styles.guidTaskReceiptLabel}>
              {t('guid.taskReceipt.context', { defaultValue: '上下文' })}
            </span>
            <span className={styles.guidTaskReceiptValue}>
              {receipt.context === 'workspace'
                ? t('guid.taskReceipt.contextWorkspace', { defaultValue: '已关联项目' })
                : t('guid.taskReceipt.contextNone', { defaultValue: '默认工作区' })}
            </span>
          </div>
          <div className={styles.guidTaskReceiptRow}>
            <span className={styles.guidTaskReceiptLabel}>
              {t('guid.taskReceipt.artifact', { defaultValue: '预计产物' })}
            </span>
            <span className={styles.guidTaskReceiptValue}>
              {t(receipt.expectedArtifactKey, { defaultValue: receipt.expectedArtifactDefault })}
            </span>
          </div>
        </div>
      ) : null}
      <button
        type='button'
        className={`${styles.guidReadinessStrip}${readiness.ready ? '' : ` ${styles.guidReadinessStripBlocked}`}`}
        data-testid='guid-readiness-strip'
        onClick={handleClick}
        aria-label={
          readiness.primaryAction === 'addModel'
            ? t('guid.readiness.addModelAria', { defaultValue: '添加模型' })
            : readiness.primaryAction === 'linkWorkspace'
              ? t('guid.readiness.linkWorkspaceAria', { defaultValue: '选择项目文件夹' })
              : t('guid.readiness.openSettings')
        }
      >
        <CheckOne theme='filled' size='14' fill='currentColor' />
        <span className={styles.guidReadinessPrimary}>{primaryText}</span>
        <span className={styles.guidReadinessMeta}>{metaText}</span>
        <SettingConfig theme='outline' size='14' fill='currentColor' />
      </button>
    </div>
  );
};

export default GuidReadinessStrip;
