import { Ban, Check, ChevronRight, Clock, Minus, TriangleAlert, X } from 'lucide-react';
import React from 'react';
import styles from './toolChips.module.css';

export type ToolChipStatus = 'pending' | 'running' | 'completed' | 'error' | 'canceled' | 'skipped' | 'invalid_arguments';

export type ToolChipLayout = 'row' | 'stack';

export type ToolChipItem = {
  id: string;
  name: string;
  detail?: string;
  status: ToolChipStatus;
  expandable?: boolean;
  expanded?: boolean;
  onToggle?: () => void;
};

export type ToolChipsProps = {
  items: ToolChipItem[];
  layout?: ToolChipLayout;
};

const statusClass = (status: ToolChipStatus): string => {
  switch (status) {
    case 'pending':
      return styles.chipPending;
    case 'running':
      return styles.chipRunning;
    case 'completed':
      return styles.chipCompleted;
    case 'error':
      return styles.chipError;
    case 'canceled':
      return styles.chipCanceled;
    case 'skipped':
      return styles.chipSkipped;
    case 'invalid_arguments':
      return styles.chipInvalid;
    default: {
      const exhaustive: never = status;
      return exhaustive;
    }
  }
};

const statusIcon = (status: ToolChipStatus): React.ReactNode => {
  const props = { size: 12, strokeWidth: 1.75, 'aria-hidden': true as const };
  switch (status) {
    case 'pending':
      return <Clock {...props} />;
    case 'running':
      return <span className={styles.spin} />;
    case 'completed':
      return <Check {...props} />;
    case 'error':
      return <X {...props} />;
    case 'canceled':
      return <Minus {...props} />;
    case 'skipped':
      return <Ban {...props} />;
    case 'invalid_arguments':
      return <TriangleAlert {...props} />;
    default: {
      const exhaustive: never = status;
      return exhaustive;
    }
  }
};

export const ToolChip: React.FC<ToolChipItem> = ({
  id,
  name,
  detail,
  status,
  expandable,
  expanded,
  onToggle,
}) => {
  const canToggle = Boolean(expandable || onToggle);
  const className = `${styles.chip} ${statusClass(status)}`;
  const content = (
    <>
      <span className={styles.icon} aria-hidden='true'>
        {statusIcon(status)}
      </span>
      <span className={`${styles.name} ${status === 'running' ? styles.shimmer : ''}`}>{name}</span>
      {detail ? (
        <span className={styles.detail} title={detail}>
          {detail}
        </span>
      ) : null}
      {canToggle ? (
        <span className={`${styles.arrow} ${expanded ? styles.arrowExpanded : ''}`} aria-hidden='true'>
          <ChevronRight size={10} strokeWidth={1.75} />
        </span>
      ) : null}
    </>
  );

  if (canToggle) {
    return (
      <button
        type='button'
        className={className}
        data-testid='beautiful-ui-tool-chip'
        data-status={status}
        data-chip-id={id}
        aria-expanded={expanded ?? false}
        onClick={onToggle}
      >
        {content}
      </button>
    );
  }

  return (
    <span className={className} data-testid='beautiful-ui-tool-chip' data-status={status} data-chip-id={id}>
      {content}
    </span>
  );
};

const ToolChips: React.FC<ToolChipsProps> = ({ items, layout = 'row' }) => {
  return (
    <div
      className={`${styles.root} ${layout === 'stack' ? styles.stack : styles.row}`}
      data-testid='beautiful-ui-tool-chips'
      data-layout={layout}
    >
      {items.map((item) => (
        <ToolChip key={item.id} {...item} />
      ))}
    </div>
  );
};

export default ToolChips;
