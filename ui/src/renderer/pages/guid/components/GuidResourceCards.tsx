/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React from 'react';
import { useTranslation } from 'react-i18next';
import styles from '../index.module.css';

const INTENTS = [
  {
    id: 'fix-code',
    textKey: 'guid.taskIntents.fixCode',
    defaultText: '分析这个项目的失败测试，修好后告诉我根因',
  },
  {
    id: 'summarize',
    textKey: 'guid.taskIntents.summarize',
    defaultText: '阅读当前工作区，用要点总结架构与风险',
  },
  {
    id: 'automate',
    textKey: 'guid.taskIntents.automate',
    defaultText: '帮我把重复手工步骤整理成可自动执行的流程',
  },
] as const;

type GuidResourceCardsProps = {
  onStartLocalAgent?: () => void;
  onSetInput?: (text: string) => void;
};

/**
 * Guid empty-area: task intents that fill the composer (first-success path).
 */
const GuidResourceCards: React.FC<GuidResourceCardsProps> = ({ onStartLocalAgent, onSetInput }) => {
  const { t } = useTranslation();

  return (
    <div className={styles.guidResourceCards} data-testid='guid-resource-cards'>
      <p className={styles.guidResourceHint}>
        {t('guid.taskIntents.hint', { defaultValue: '不知道从哪开始？点一条直接填入输入框：' })}
      </p>
      <div className={styles.guidResourceIntentRow}>
        {INTENTS.map((intent) => (
          <button
            key={intent.id}
            type='button'
            className={styles.guidResourceIntentChip}
            data-testid={`guid-intent-${intent.id}`}
            onClick={() => {
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
