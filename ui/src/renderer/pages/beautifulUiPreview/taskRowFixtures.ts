import type { TaskRowItem, TaskRowStatus } from '@renderer/components/beautifulUi/taskRows/TaskRows';
import type { TFunction } from 'i18next';
import { TASK_ROW_STATUSES, type TaskRowContentMode } from './catalog';

const trailingStatus = (status: TaskRowStatus): TaskRowStatus => {
  switch (status) {
    case 'completed':
      return 'completed';
    case 'canceled':
      return 'canceled';
    case 'failed':
      return 'waiting';
    case 'running':
    case 'waiting':
      return 'waiting';
    default: {
      const exhaustive: never = status;
      return exhaustive;
    }
  }
};

export const buildTaskRowItems = (
  status: TaskRowStatus,
  content: TaskRowContentMode,
  t: TFunction
): TaskRowItem[] => {
  if (content === 'empty') return [];
  if (content === 'long') {
    return [
      {
        id: 'long',
        title: t('beautifulUiPreview.fixtures.taskRows.long.title'),
        detail: t('beautifulUiPreview.fixtures.taskRows.long.detail'),
        status,
      },
    ];
  }
  if (content === 'mixed') {
    return TASK_ROW_STATUSES.map((rowStatus) => ({
      id: rowStatus,
      title: t('beautifulUiPreview.fixtures.taskRows.reorder.title'),
      detail: t(`beautifulUiPreview.taskRowStatuses.${rowStatus}` as const),
      status: rowStatus,
    }));
  }

  return [
    {
      id: 'verify',
      title: t('beautifulUiPreview.fixtures.taskRows.verify.title'),
      detail: t('beautifulUiPreview.fixtures.taskRows.verify.detail'),
      status: 'completed',
      children: [
        {
          id: 'ids',
          title: t('beautifulUiPreview.fixtures.taskRows.verify.ids'),
          detail: t('beautifulUiPreview.fixtures.taskRows.verify.idsDetail'),
          status: 'completed',
        },
        {
          id: 'stale',
          title: t('beautifulUiPreview.fixtures.taskRows.verify.stale'),
          detail: t('beautifulUiPreview.fixtures.taskRows.verify.staleDetail'),
          status: 'completed',
        },
      ],
    },
    {
      id: 'reorder',
      title: t('beautifulUiPreview.fixtures.taskRows.reorder.title'),
      detail: t('beautifulUiPreview.fixtures.taskRows.reorder.detail'),
      status,
      children: [
        {
          id: 'read',
          title: t('beautifulUiPreview.fixtures.taskRows.reorder.read'),
          detail: t('beautifulUiPreview.fixtures.taskRows.reorder.readDetail'),
          status,
        },
        {
          id: 'score',
          title: t('beautifulUiPreview.fixtures.taskRows.reorder.score'),
          detail: t('beautifulUiPreview.fixtures.taskRows.reorder.scoreDetail'),
          status: trailingStatus(status),
        },
      ],
    },
    {
      id: 'draft',
      title: t('beautifulUiPreview.fixtures.taskRows.draft.title'),
      detail: t('beautifulUiPreview.fixtures.taskRows.draft.detail'),
      status: trailingStatus(status),
      children: [
        {
          id: 'cone',
          title: t('beautifulUiPreview.fixtures.taskRows.draft.cone'),
          detail: t('beautifulUiPreview.fixtures.taskRows.draft.coneDetail'),
          status: trailingStatus(status),
        },
        {
          id: 'pistachio',
          title: t('beautifulUiPreview.fixtures.taskRows.draft.pistachio'),
          detail: t('beautifulUiPreview.fixtures.taskRows.draft.pistachioDetail'),
          status: trailingStatus(status),
        },
      ],
    },
  ];
};
