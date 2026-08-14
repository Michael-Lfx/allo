import React from 'react';
import { useTranslation } from 'react-i18next';
import styles from './recommendationCard.module.css';

export type RecommendationTone = 'high' | 'review' | 'alternatives' | 'none';
export type RecommendationAction = { id: string; label: string; onClick: () => void };
export type RecommendationCardProps = {
  title: string;
  body: React.ReactNode;
  tone: RecommendationTone;
  actions: RecommendationAction[];
  alternatives?: RecommendationAction[];
};

const toneLabelKey = (tone: RecommendationTone) => {
  switch (tone) {
    case 'high':
      return 'beautifulUiPreview.recommendationTones.high' as const;
    case 'review':
      return 'beautifulUiPreview.recommendationTones.review' as const;
    case 'alternatives':
      return 'beautifulUiPreview.recommendationTones.alternatives' as const;
    case 'none':
      return 'beautifulUiPreview.recommendationTones.none' as const;
    default: {
      const exhaustive: never = tone;
      return exhaustive;
    }
  }
};

const RecommendationCard: React.FC<RecommendationCardProps> = ({
  title,
  body,
  tone,
  actions,
  alternatives,
}) => {
  const { t } = useTranslation();

  return (
    <div className={styles.card} data-testid='beautiful-ui-recommendation-card' data-tone={tone}>
      <div className={styles.header}>
        <p className={styles.title}>{title}</p>
        <span className={styles.tone} data-tone={tone}>
          {t(toneLabelKey(tone))}
        </span>
      </div>
      {body ? <div className={styles.body}>{body}</div> : null}
      {alternatives && alternatives.length > 0 ? (
        <ul className={styles.alternatives}>
          {alternatives.map((item) => (
            <li key={item.id}>
              <button type='button' className={styles.alternative} onClick={item.onClick}>
                {item.label}
              </button>
            </li>
          ))}
        </ul>
      ) : null}
      {actions.length > 0 ? (
        <div className={styles.actions}>
          {actions.map((action, index) => (
            <button
              key={action.id}
              type='button'
              className={index === 0 ? styles.primary : styles.secondary}
              onClick={action.onClick}
            >
              {action.label}
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
};

export default RecommendationCard;
