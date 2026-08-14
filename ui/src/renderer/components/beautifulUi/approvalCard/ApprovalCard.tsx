import { CircleCheck, Info, Pencil, Plug, Terminal } from 'lucide-react';
import React from 'react';
import styles from './approvalCard.module.css';

export type ApprovalKind = 'edit' | 'exec' | 'info' | 'mcp' | 'plan';

export type ApprovalOption = { id: string; label: string };

export type ApprovalCardProps = {
  title: string;
  kind: ApprovalKind;
  options: ApprovalOption[];
  selectedId?: string | null;
  onSelect: (id: string) => void;
  onConfirm: () => void;
  confirmLabel: string;
  disabled?: boolean;
  description?: string;
  children?: React.ReactNode;
};

const kindIcon = (kind: ApprovalKind): React.ReactNode => {
  const props = { size: 16, strokeWidth: 1.75, 'aria-hidden': true as const };
  switch (kind) {
    case 'edit':
      return <Pencil {...props} />;
    case 'exec':
      return <Terminal {...props} />;
    case 'info':
      return <Info {...props} />;
    case 'mcp':
      return <Plug {...props} />;
    case 'plan':
      return <CircleCheck {...props} />;
    default: {
      const exhaustive: never = kind;
      return exhaustive;
    }
  }
};

const ApprovalCard: React.FC<ApprovalCardProps> = ({
  title,
  kind,
  options,
  selectedId,
  onSelect,
  onConfirm,
  confirmLabel,
  disabled,
  description,
  children,
}) => {
  const confirmDisabled = !selectedId || disabled;

  return (
    <div className={styles.card} data-testid='beautiful-ui-approval-card' data-kind={kind}>
      <div className={styles.header}>
        <span className={styles.icon} aria-hidden='true'>
          {kindIcon(kind)}
        </span>
        <div className={styles.copy}>
          <p className={styles.title}>{title}</p>
          {description ? <p className={styles.description}>{description}</p> : null}
        </div>
      </div>
      {children ? <div className={styles.body}>{children}</div> : null}
      {options.length > 0 ? (
        <>
          <div className={styles.options} role='listbox' aria-label={title}>
            {options.map((option) => {
              const selected = option.id === selectedId;
              return (
                <button
                  key={option.id}
                  type='button'
                  role='option'
                  aria-selected={selected}
                  className={`${styles.option} ${selected ? styles.optionSelected : ''}`}
                  onClick={() => onSelect(option.id)}
                  disabled={disabled}
                >
                  <span className={styles.optionMark} aria-hidden='true' />
                  <span className={styles.optionLabel}>{option.label}</span>
                </button>
              );
            })}
          </div>
          <button
            type='button'
            className={styles.confirm}
            disabled={confirmDisabled}
            onClick={onConfirm}
          >
            {confirmLabel}
          </button>
        </>
      ) : null}
    </div>
  );
};

export default ApprovalCard;
