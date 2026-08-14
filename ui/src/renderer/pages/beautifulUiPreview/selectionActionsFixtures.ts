import type { SelectionAction } from '@renderer/components/beautifulUi/selectionActions/SelectionActions';
import {
  BEAUTIFUL_UI_SELECTION_ACTION_IDS,
  selectionActionLabelKey,
} from '@renderer/components/beautifulUi/selectionActions/selectionActionsModel';
import type { TFunction } from 'i18next';

export type SelectionActionsFixture = {
  top: number;
  left: number;
  sample: string;
  actions: SelectionAction[];
};

const noop = (): void => undefined;

export const buildSelectionActionsFixture = (t: TFunction): SelectionActionsFixture => ({
  top: 16,
  left: 160,
  sample: t('beautifulUiPreview.fixtures.selectionActions.sample'),
  actions: BEAUTIFUL_UI_SELECTION_ACTION_IDS.map((id) => ({
    id,
    label: t(selectionActionLabelKey(id)),
    onClick: noop,
  })),
});
