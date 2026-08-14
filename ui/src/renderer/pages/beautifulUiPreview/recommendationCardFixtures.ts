import type {
  RecommendationAction,
  RecommendationCardProps,
  RecommendationTone,
} from '@renderer/components/beautifulUi/recommendationCard/RecommendationCard';
import type { TFunction } from 'i18next';
import type { RecommendationPreviewVariant } from './catalog';

export type RecommendationCardFixture = Omit<RecommendationCardProps, 'body'> & {
  body: string;
};

const noop = (): void => undefined;

const acceptAction = (t: TFunction): RecommendationAction => ({
  id: 'accept',
  label: t('beautifulUiPreview.fixtures.recommendationCard.actions.accept'),
  onClick: noop,
});

const dismissAction = (t: TFunction): RecommendationAction => ({
  id: 'dismiss',
  label: t('beautifulUiPreview.fixtures.recommendationCard.actions.dismiss'),
  onClick: noop,
});

const alternativeActions = (t: TFunction): RecommendationAction[] => [
  {
    id: 'mint',
    label: t('beautifulUiPreview.fixtures.recommendationCard.alternatives.mint'),
    onClick: noop,
  },
  {
    id: 'restock',
    label: t('beautifulUiPreview.fixtures.recommendationCard.alternatives.restock'),
    onClick: noop,
  },
];

const toneForVariant = (variant: RecommendationPreviewVariant): RecommendationTone => {
  switch (variant) {
    case 'high':
    case 'accept':
      return 'high';
    case 'alternatives':
      return 'alternatives';
    case 'review':
      return 'review';
    case 'none':
      return 'none';
    default: {
      const exhaustive: never = variant;
      return exhaustive;
    }
  }
};

export const buildRecommendationCardFixture = (
  variant: RecommendationPreviewVariant,
  t: TFunction
): RecommendationCardFixture => {
  const title = t('beautifulUiPreview.fixtures.recommendationCard.title');
  const body = t('beautifulUiPreview.fixtures.recommendationCard.body');
  const tone = toneForVariant(variant);

  switch (variant) {
    case 'high':
      return { title, body, tone, actions: [acceptAction(t), dismissAction(t)] };
    case 'alternatives':
      return {
        title,
        body,
        tone,
        actions: [acceptAction(t)],
        alternatives: alternativeActions(t),
      };
    case 'accept':
      return { title, body, tone, actions: [acceptAction(t)] };
    case 'review':
      return { title, body, tone, actions: [acceptAction(t), dismissAction(t)] };
    case 'none':
      return { title, body: '', tone, actions: [] };
    default: {
      const exhaustive: never = variant;
      return exhaustive;
    }
  }
};
