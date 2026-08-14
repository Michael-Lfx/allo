import type { SelectionActionId } from './SelectionActions';

export type { SelectionActionId };

export const BEAUTIFUL_UI_SELECTION_ACTION_IDS = [
  'explain',
  'improve',
  'shorten',
  'tone',
  'grammar',
] as const;

export type BeautifulUiSelectionActionId = (typeof BEAUTIFUL_UI_SELECTION_ACTION_IDS)[number];

export const selectionActionLabelKey = (id: SelectionActionId) => {
  switch (id) {
    case 'explain':
      return 'beautifulUiPreview.fixtures.selectionActions.explain' as const;
    case 'improve':
      return 'beautifulUiPreview.fixtures.selectionActions.improve' as const;
    case 'shorten':
      return 'beautifulUiPreview.fixtures.selectionActions.shorten' as const;
    case 'tone':
      return 'beautifulUiPreview.fixtures.selectionActions.tone' as const;
    case 'grammar':
      return 'beautifulUiPreview.fixtures.selectionActions.grammar' as const;
    case 'quote':
      return 'common.reply' as const;
    default: {
      const exhaustive: never = id;
      return exhaustive;
    }
  }
};
