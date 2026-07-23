

import type { TProviderWithModel } from '@/common/config/storage';
import { Attention, CheckOne, SettingConfig } from '@icon-park/react';
import React, { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import styles from '../index.module.css';
import {
  buildGuidStatusChips,
  type GuidReadinessResult,
  type GuidTaskReceipt,
} from '../readiness/guidReadiness';

type GuidReadinessStripProps = {
  agentLabel: string;
  model?: TProviderWithModel | null;
  workspaceDir?: string;
  readiness: GuidReadinessResult;
  receipt: GuidTaskReceipt | null;
  hasDraft: boolean;
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
  hasDraft,
  onOpenSettings,
  onAddModel,
  onLinkWorkspace,
}) => {
  const { t } = useTranslation();
  const modelLabel = model?.use_model || model?.name;
  const chips = useMemo(
    () => buildGuidStatusChips({ readiness, hasDraft: hasDraft || Boolean(receipt) }),
    [hasDraft, readiness, receipt]
  );

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
    if (hasDraft && readiness.blocker === 'model') {
      return t('guid.readiness.draftHeldModel', {
        defaultValue: '草稿已保留 · 连接模型后将自动继续',
      });
    }
    if (hasDraft && readiness.blocker === 'workspace') {
      return t('guid.readiness.draftHeldWorkspace', {
        defaultValue: '草稿已保留 · 选择项目后将自动继续',
      });
    }
    if (receipt) {
      return t(receipt.expectedArtifactKey, { defaultValue: receipt.expectedArtifactDefault });
    }
    if (workspaceDir) return t('guid.readiness.workspaceLinked');
    if (readiness.requiresWorkspace) return t('guid.readiness.workspaceRequired');
    return t('guid.readiness.workspaceOptional');
  })();

  return (
    <div className={styles.guidReadinessBlock} data-testid='guid-readiness-block'>
      <div className={styles.guidStatusChips} data-testid='guid-status-chips' aria-label={t('guid.status.rowAria', { defaultValue: '任务就绪状态' })}>
        {chips.map((chip) => (
          <span
            key={chip.id}
            className={`${styles.guidStatusChip} ${styles[`guidStatusChip_${chip.state}`]}`}
            data-testid={`guid-status-${chip.id}`}
            data-state={chip.state}
          >
            {t(chip.labelKey, { defaultValue: chip.defaultLabel })}
          </span>
        ))}
      </div>

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
              {t('guid.taskReceipt.plan', { defaultValue: '执行方案' })}
            </span>
            <span className={styles.guidTaskReceiptValue} data-testid='guid-execution-preview'>
              {receipt.planSteps
                .map((step) => t(step.key, { defaultValue: step.defaultLabel }))
                .join(' → ')}
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
              : t('guid.readiness.openPlan', { defaultValue: '打开执行方案详情' })
        }
      >
        {readiness.ready ? (
          <CheckOne theme='filled' size='14' fill='currentColor' />
        ) : (
          <Attention theme='filled' size='14' fill='currentColor' />
        )}
        <span className={styles.guidReadinessPrimary}>{primaryText}</span>
        <span className={styles.guidReadinessMeta}>{metaText}</span>
        <SettingConfig theme='outline' size='14' fill='currentColor' />
      </button>
    </div>
  );
};

export default GuidReadinessStrip;
