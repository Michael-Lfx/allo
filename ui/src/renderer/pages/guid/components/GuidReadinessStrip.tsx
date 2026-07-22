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

type GuidReadinessStripProps = {
  agentLabel: string;
  model?: TProviderWithModel | null;
  workspaceDir?: string;
  onOpenSettings: () => void;
};

const GuidReadinessStrip: React.FC<GuidReadinessStripProps> = ({
  agentLabel,
  model,
  workspaceDir,
  onOpenSettings,
}) => {
  const { t } = useTranslation();
  const modelLabel = model?.use_model || model?.name;

  return (
    <button
      type='button'
      className={styles.guidReadinessStrip}
      data-testid='guid-readiness-strip'
      onClick={onOpenSettings}
      aria-label={t('guid.readiness.openSettings')}
    >
      <CheckOne theme='filled' size='14' fill='currentColor' />
      <span className={styles.guidReadinessPrimary}>
        {modelLabel
          ? t('guid.readiness.ready', { agent: agentLabel, model: modelLabel })
          : t('guid.readiness.needsModel', { agent: agentLabel })}
      </span>
      <span className={styles.guidReadinessMeta}>
        {workspaceDir ? t('guid.readiness.workspaceLinked') : t('guid.readiness.workspaceOptional')}
      </span>
      <SettingConfig theme='outline' size='14' fill='currentColor' />
    </button>
  );
};

export default GuidReadinessStrip;
