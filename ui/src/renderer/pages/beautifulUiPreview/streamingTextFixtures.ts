import type { TFunction } from 'i18next';
import type { StreamingTextContentMode } from './catalog';

export type StreamingTextFixture = {
  body: string;
  sourcesLabel?: string;
  followUps: string[];
};

export const buildStreamingTextFixture = (
  content: StreamingTextContentMode,
  t: TFunction
): StreamingTextFixture => {
  switch (content) {
    case 'empty':
      return { body: '', followUps: [] };
    case 'long':
      return {
        body: t('beautifulUiPreview.fixtures.streamingText.long'),
        sourcesLabel: t('beautifulUiPreview.fixtures.streamingText.sources'),
        followUps: [
          t('beautifulUiPreview.fixtures.streamingText.followUps.scoops'),
          t('beautifulUiPreview.fixtures.streamingText.followUps.churn'),
        ],
      };
    case 'typical':
      return {
        body: t('beautifulUiPreview.fixtures.streamingText.typical'),
        sourcesLabel: t('beautifulUiPreview.fixtures.streamingText.sources'),
        followUps: [
          t('beautifulUiPreview.fixtures.streamingText.followUps.scoops'),
          t('beautifulUiPreview.fixtures.streamingText.followUps.churn'),
        ],
      };
    default: {
      const exhaustive: never = content;
      return exhaustive;
    }
  }
};
