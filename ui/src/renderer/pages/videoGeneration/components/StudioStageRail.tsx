/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React from 'react';
import { useTranslation } from 'react-i18next';
import { Check } from '@icon-park/react';
import type { VimaxRunStatus } from '../types';
import styles from '../index.module.css';

interface StudioStageRailProps {
  status?: VimaxRunStatus | null;
  stage?: string | null;
  hasStoryboard: boolean;
  hasFinalVideo: boolean;
}

const StudioStageRail: React.FC<StudioStageRailProps> = ({
  status,
  stage,
  hasStoryboard,
  hasFinalVideo,
}) => {
  const { t } = useTranslation();
  const activeIndex = hasFinalVideo
    ? 3
    : status === 'rendering'
      ? 2
      : hasStoryboard || stage === 'planned'
        ? 1
        : 0;
  const labels = [
    t('videoGeneration.studio.stages.brief', { defaultValue: '创意' }),
    t('videoGeneration.studio.stages.storyboard', { defaultValue: '分镜' }),
    t('videoGeneration.studio.stages.render', { defaultValue: '渲染' }),
    t('videoGeneration.studio.stages.film', { defaultValue: '成片' }),
  ];

  return (
    <nav
      className={styles.stageRail}
      aria-label={t('videoGeneration.studio.stageLabel', { defaultValue: '影片制作进度' })}
    >
      {labels.map((label, index) => {
        const done = index < activeIndex;
        const current = index === activeIndex;
        return (
          <div
            key={label}
            className={[
              styles.stageItem,
              done || current ? styles.stageItemActive : '',
              done ? styles.stageItemDone : '',
              current ? styles.stageItemCurrent : '',
            ].join(' ')}
            aria-current={current ? 'step' : undefined}
          >
            <span className={styles.stageDot}>
              {done ? <Check theme='outline' size={12} strokeWidth={4} /> : index + 1}
            </span>
            <span className={styles.stageLabel}>{label}</span>
          </div>
        );
      })}
    </nav>
  );
};

export default StudioStageRail;
