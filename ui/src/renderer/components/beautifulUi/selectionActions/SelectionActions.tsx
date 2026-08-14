import { MessageCircleQuestion, Quote, Scissors, Smile, Sparkles, Type } from 'lucide-react';
import React from 'react';
import styles from './selectionActions.module.css';

export type SelectionActionId = 'explain' | 'improve' | 'shorten' | 'tone' | 'grammar' | 'quote';
export type SelectionAction = { id: SelectionActionId; label: string; onClick: () => void };
export type SelectionActionsProps = {
  top: number;
  left: number;
  actions: SelectionAction[];
};

const actionIcon = (id: SelectionActionId): React.ReactNode => {
  const props = { size: 12, strokeWidth: 1.75, 'aria-hidden': true as const };
  switch (id) {
    case 'explain':
      return <MessageCircleQuestion {...props} />;
    case 'improve':
      return <Sparkles {...props} />;
    case 'shorten':
      return <Scissors {...props} />;
    case 'tone':
      return <Smile {...props} />;
    case 'grammar':
      return <Type {...props} />;
    case 'quote':
      return <Quote {...props} />;
    default: {
      const exhaustive: never = id;
      return exhaustive;
    }
  }
};

const SelectionActions: React.FC<SelectionActionsProps> = ({ top, left, actions }) => (
  <div
    className={styles.toolbar}
    data-testid='beautiful-ui-selection-actions'
    role='toolbar'
    style={{ top, left }}
    onMouseDown={(event) => event.preventDefault()}
  >
    {actions.map((action) => (
      <button
        key={action.id}
        type='button'
        className={styles.action}
        data-action-id={action.id}
        onClick={action.onClick}
      >
        {actionIcon(action.id)}
        {action.label}
      </button>
    ))}
  </div>
);

export default SelectionActions;
