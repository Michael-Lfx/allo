import type { DiffTableFile } from '@renderer/components/beautifulUi/diffTable/DiffTable';
import type { TFunction } from 'i18next';

export type DiffTableFixture = {
  title: string;
  files: DiffTableFile[];
};

export const buildDiffTableFixture = (t: TFunction): DiffTableFixture => ({
  title: t('beautifulUiPreview.fixtures.diffTable.title'),
  files: [
    {
      id: t('beautifulUiPreview.fixtures.diffTable.churn.path'),
      title: t('beautifulUiPreview.fixtures.diffTable.churn.title'),
      insertions: 18,
      deletions: 3,
    },
    {
      id: t('beautifulUiPreview.fixtures.diffTable.reorder.path'),
      title: t('beautifulUiPreview.fixtures.diffTable.reorder.title'),
      insertions: 12,
      deletions: 2,
    },
  ],
});
