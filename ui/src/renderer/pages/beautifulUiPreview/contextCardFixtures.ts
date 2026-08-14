import type { ContextCardItem } from '@renderer/components/beautifulUi/contextCards/ContextCards';
import type { TFunction } from 'i18next';
import type { ContextCardContentMode } from './catalog';

export const buildContextCardItems = (
  content: ContextCardContentMode,
  t: TFunction
): ContextCardItem[] => {
  switch (content) {
    case 'empty':
      return [];
    case 'typical':
      return [
        {
          id: 'pdf',
          title: t('beautifulUiPreview.fixtures.contextCards.pdf.title'),
          snippet: t('beautifulUiPreview.fixtures.contextCards.pdf.snippet'),
          sourceKind: 'pdf',
          sourceLabel: t('beautifulUiPreview.fixtures.contextCards.pdf.source'),
        },
        {
          id: 'csv',
          title: t('beautifulUiPreview.fixtures.contextCards.csv.title'),
          snippet: t('beautifulUiPreview.fixtures.contextCards.csv.snippet'),
          sourceKind: 'csv',
          sourceLabel: t('beautifulUiPreview.fixtures.contextCards.csv.source'),
        },
      ];
    default: {
      const exhaustive: never = content;
      return exhaustive;
    }
  }
};
