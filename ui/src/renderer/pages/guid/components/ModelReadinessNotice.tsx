import { Attention } from '@icon-park/react';
import React from 'react';
import { useTranslation } from 'react-i18next';
import styles from '../index.module.css';

export type ModelReadinessNoticeProps = {
  state: 'missing' | 'unavailable';
  hasDraft: boolean;
  onChooseModel: () => void;
};

const ModelReadinessNotice: React.FC<ModelReadinessNoticeProps> = ({ state, hasDraft, onChooseModel }) => {
  const { t } = useTranslation();
  const message = state === 'missing'
    ? t('conversation.chat.noModelSelected')
    : t('conversation.chat.modelUnavailableHint');

  return (
    <div
      className={`${styles.modelReadinessNotice} ${hasDraft ? styles.modelReadinessNoticeDraft : ''}`}
      role='alert'
      aria-live='polite'
      data-testid='guid-model-readiness-notice'
      data-state={state}
    >
      <Attention theme='filled' size='14' fill='currentColor' aria-hidden='true' />
      <span className={styles.modelReadinessNoticeText}>{message}</span>
      {hasDraft ? <span className={styles.modelReadinessNoticeMeta}>{t('guid.readiness.draftHeldModel')}</span> : null}
      <button type='button' className={styles.modelReadinessNoticeAction} onClick={onChooseModel}>
        {t('conversation.chat.openModelSettings')}
      </button>
    </div>
  );
};

export default ModelReadinessNotice;
