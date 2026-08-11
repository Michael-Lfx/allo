import React from 'react';
import { useTranslation } from 'react-i18next';
import { Check } from '@icon-park/react';
import type { BoardStage } from '../types';
import { stageLabel } from '../stageI18n';
import styles from '../index.module.css';

interface StudioStageRailProps {
  stages: BoardStage[];
  currentStage?: string | null;
  awaitingHumanStage?: string | null;
  onSelectStage?: (stageName: string) => void;
  selectedStage?: string | null;
}

function stageDone(status: string): boolean {
  return status === 'completed';
}

function stageCurrent(
  stage: BoardStage,
  currentStage?: string | null,
  awaitingHumanStage?: string | null
): boolean {
  if (awaitingHumanStage && stage.name === awaitingHumanStage) return true;
  if (currentStage && stage.name === currentStage) return true;
  return stage.status === 'in_progress' || stage.status === 'awaiting_human';
}

const StudioStageRail: React.FC<StudioStageRailProps> = ({
  stages,
  currentStage,
  awaitingHumanStage,
  onSelectStage,
  selectedStage,
}) => {
  const { t } = useTranslation();

  if (stages.length === 0) {
    return (
      <nav
        className={styles.stageRail}
        aria-label={t('videoGeneration.studio.stageLabel', { defaultValue: '影片制作进度' })}
      >
        <div className={styles.stageItem}>
          <span className={styles.stageDot}>1</span>
          <span className={styles.stageLabel}>
            {t('videoGeneration.studio.stages.empty', { defaultValue: '待开始' })}
          </span>
        </div>
      </nav>
    );
  }

  return (
    <nav
      className={styles.stageRail}
      aria-label={t('videoGeneration.studio.stageLabel', { defaultValue: '影片制作进度' })}
    >
      {stages.map((stage, index) => {
        const done = stageDone(String(stage.status));
        const current = stageCurrent(stage, currentStage, awaitingHumanStage);
        const selected = selectedStage === stage.name;
        const label = stageLabel(stage.name, t);
        return (
          <button
            key={stage.name}
            type='button'
            className={[
              styles.stageItem,
              done || current || selected ? styles.stageItemActive : '',
              done ? styles.stageItemDone : '',
              current || selected ? styles.stageItemCurrent : '',
            ]
              .filter(Boolean)
              .join(' ')}
            aria-current={current ? 'step' : undefined}
            onClick={() => onSelectStage?.(stage.name)}
          >
            <span className={styles.stageDot}>
              {done ? <Check theme='outline' size={12} strokeWidth={4} /> : index + 1}
            </span>
            <span className={styles.stageLabel}>{label}</span>
          </button>
        );
      })}
    </nav>
  );
};

export default StudioStageRail;
