/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React from 'react';
import { useTranslation } from 'react-i18next';
import styles from '../index.module.css';
import { GUID_TASK_INTENTS, type GuidTaskIntentId } from '../readiness/guidReadiness';

type GuidResourceCardsProps = {
  onStartLocalAgent?: () => void;
  onSetInput?: (text: string) => void;
  onSelectIntent?: (intentId: GuidTaskIntentId) => void;
  activeIntentId?: GuidTaskIntentId;
};

/**
 * Guid empty-area: task intents that fill the composer (first-success path).
 */
const GuidResourceCards: React.FC<GuidResourceCardsProps> = ({
  onStartLocalAgent,
  onSetInput,
  onSelectIntent,
  activeIntentId,
}) => {
  const { t } = useTranslation();

  return (
    <div className={styles.guidResourceCards} data-testid='guid-resource-cards'>
      <p className={styles.guidResourceHint}>
        {t('guid.taskIntents.hint', { defaultValue: '从一个真实任务开始' })}
      </p>
      <div className={styles.guidResourceIntentRow}>
        {GUID_TASK_INTENTS.map((intent) => (
          <button
            key={intent.id}
            type='button'
            className={`${styles.guidResourceIntentChip}${activeIntentId === intent.id ? ` ${styles.guidResourceIntentChipActive}` : ''}`}
            data-testid={`guid-intent-${intent.id}`}
            aria-pressed={activeIntentId === intent.id}
            onClick={() => {
              onSelectIntent?.(intent.id);
              onSetInput?.(t(intent.textKey, { defaultValue: intent.defaultText }));
              onStartLocalAgent?.();
            }}
          >
            {t(intent.textKey, { defaultValue: intent.defaultText })}
          </button>
        ))}
      </div>
    </div>
  );
};

export default GuidResourceCards;
