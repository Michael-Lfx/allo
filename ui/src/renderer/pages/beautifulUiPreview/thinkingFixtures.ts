import type { ThinkingTraceItem, ThinkingTraceItemState, ThinkingTraceStatus, ThinkingTraceVariant } from '@renderer/components/beautifulUi/thinking/ThinkingTrace';
import type { TFunction } from 'i18next';
import type { ThinkingContentMode } from './catalog';

const itemStateForStatus = (status: ThinkingTraceStatus, live: ThinkingTraceItemState): ThinkingTraceItemState => {
  switch (status) {
    case 'thinking':
      return live;
    case 'waiting':
      return 'pending';
    case 'done':
    case 'failed':
    case 'canceled':
      return 'done';
    default: {
      const exhaustive: never = status;
      return exhaustive;
    }
  }
};

const typicalItems = (
  variant: ThinkingTraceVariant,
  status: ThinkingTraceStatus,
  t: TFunction
): ThinkingTraceItem[] => {
  const running = itemStateForStatus(status, 'running');
  const pending = itemStateForStatus(status, 'pending');

  switch (variant) {
    case 'steps':
      return [
        {
          id: 's1',
          title: t('beautifulUiPreview.fixtures.steps.1.title'),
          detail: t('beautifulUiPreview.fixtures.steps.1.detail'),
          state: 'done',
        },
        {
          id: 's2',
          title: t('beautifulUiPreview.fixtures.steps.2.title'),
          detail: t('beautifulUiPreview.fixtures.steps.2.detail'),
          state: running,
        },
        {
          id: 's3',
          title: t('beautifulUiPreview.fixtures.steps.3.title'),
          detail: t('beautifulUiPreview.fixtures.steps.3.detail'),
          state: pending,
        },
      ];
    case 'reasoning':
      return [
        {
          id: 'r1',
          title: t('beautifulUiPreview.fixtures.reasoning.1.title'),
          detail: t('beautifulUiPreview.fixtures.reasoning.1.detail'),
          state: running,
        },
      ];
    case 'search':
      return [
        {
          id: 'q1',
          title: t('beautifulUiPreview.fixtures.search.1.title'),
          detail: t('beautifulUiPreview.fixtures.search.1.detail'),
          state: 'done',
        },
        {
          id: 'q2',
          title: t('beautifulUiPreview.fixtures.search.2.title'),
          detail: t('beautifulUiPreview.fixtures.search.2.detail'),
          state: running,
        },
      ];
    case 'coding':
      return [
        {
          id: 'c1',
          title: t('beautifulUiPreview.fixtures.coding.1.title'),
          detail: t('beautifulUiPreview.fixtures.coding.1.detail'),
          state: 'done',
        },
        {
          id: 'c2',
          title: t('beautifulUiPreview.fixtures.coding.2.title'),
          detail: t('beautifulUiPreview.fixtures.coding.2.detail'),
          state: running,
        },
      ];
    default: {
      const exhaustive: never = variant;
      return exhaustive;
    }
  }
};

export const buildThinkingItems = (
  variant: ThinkingTraceVariant,
  status: ThinkingTraceStatus,
  content: ThinkingContentMode,
  t: TFunction
): ThinkingTraceItem[] => {
  if (content === 'empty') return [];
  if (content === 'long') {
    return [
      {
        id: 'long',
        title: t('beautifulUiPreview.fixtures.long.title'),
        detail: t('beautifulUiPreview.fixtures.long.detail'),
        state: itemStateForStatus(status, 'running'),
      },
    ];
  }
  return typicalItems(variant, status, t);
};
