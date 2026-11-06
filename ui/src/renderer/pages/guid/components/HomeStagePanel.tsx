import React from 'react';
import { Close, Left } from '@icon-park/react';
import { useTranslation } from 'react-i18next';
import styles from './HomeStagePanel.module.css';

export type HomeCapability = 'office' | 'develop' | 'slides' | 'video';

export type HomeStageOption = {
  /** Stable React key; a group can expose two entries for the same capability. */
  key: string;
  id: HomeCapability;
  title: string;
  description?: string;
  disabled?: boolean;
};

export interface HomeStageFrameProps {
  title: string;
  onBack?: () => void;
  onClose: () => void;
  children: React.ReactNode;
  testId?: string;
}

/** Shared frame for the upper stage when a directory or capability prep area is open. */
export const HomeStageFrame: React.FC<HomeStageFrameProps> = ({ title, onBack, onClose, children, testId }) => {
  const { t } = useTranslation();
  return (
    <section className={styles.frame} data-home-stage-panel='true' data-testid={testId}>
      <header className={styles.header}>
        {onBack ? (
          <button type='button' className={styles.back} onClick={onBack}>
            <Left theme='outline' size='16' fill='currentColor' />
            <span>{t('workshop.back', { defaultValue: '返回' })}</span>
          </button>
        ) : null}
        <h2 className={styles.title}>{title}</h2>
        <span className={styles.spacer} />
        <button
          type='button'
          className={styles.close}
          aria-label={t('common.close', { defaultValue: '关闭' })}
          onClick={onClose}
        >
          <Close theme='outline' size='16' fill='currentColor' />
        </button>
      </header>
      <div className={styles.body}>{children}</div>
    </section>
  );
};

export const HomeStageOptions: React.FC<{
  options: HomeStageOption[];
  onSelect: (option: HomeStageOption) => void;
}> = ({ options, onSelect }) => (
  <div className={styles.options}>
    {options.map((option) => (
      <button
        key={option.key}
        type='button'
        className={styles.option}
        data-testid={`guid-home-option-${option.key}`}
        disabled={option.disabled}
        onClick={() => onSelect(option)}
      >
        <span className={styles.optionTitle}>{option.title}</span>
        {option.description ? <span className={styles.optionDescription}>{option.description}</span> : null}
      </button>
    ))}
  </div>
);
