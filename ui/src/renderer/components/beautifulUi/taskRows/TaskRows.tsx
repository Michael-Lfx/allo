import { Check, ChevronRight, Clock, Minus, X } from 'lucide-react';
import React from 'react';
import shimmerStyles from '../textShimmer.module.css';
import styles from './taskRows.module.css';

export type TaskRowStatus = 'running' | 'waiting' | 'completed' | 'failed' | 'canceled';

export type TaskRowLayout = 'capsules' | 'list';

export type TaskRowItem = {
  id: string;
  title: string;
  detail?: string;
  status: TaskRowStatus;
  children?: TaskRowItem[];
  onClick?: () => void;
};

export type TaskRowsProps = {
  items: TaskRowItem[];
  layout?: TaskRowLayout;
};

export type TaskGroupProps = {
  title: React.ReactNode;
  status: TaskRowStatus;
  expandable?: boolean;
  expanded?: boolean;
  onToggle?: () => void;
  ariaControls?: string;
  className?: string;
};

const statusClass = (status: TaskRowStatus): string => {
  switch (status) {
    case 'running':
      return styles.rowRunning;
    case 'waiting':
      return styles.rowWaiting;
    case 'completed':
      return styles.rowCompleted;
    case 'failed':
      return styles.rowFailed;
    case 'canceled':
      return styles.rowCanceled;
    default: {
      const exhaustive: never = status;
      return exhaustive;
    }
  }
};

const statusIcon = (status: TaskRowStatus): React.ReactNode => {
  const props = { size: 12, strokeWidth: 1.75, 'aria-hidden': true as const };
  switch (status) {
    case 'running':
      return <span className={styles.spin} />;
    case 'waiting':
      return <Clock {...props} />;
    case 'completed':
      return <Check {...props} />;
    case 'failed':
      return <X {...props} />;
    case 'canceled':
      return <Minus {...props} />;
    default: {
      const exhaustive: never = status;
      return exhaustive;
    }
  }
};

const RowBody: React.FC<{
  title: React.ReactNode;
  detail?: string;
  status: TaskRowStatus;
  showArrow?: boolean;
  expanded?: boolean;
}> = ({ title, detail, status, showArrow, expanded }) => (
  <>
    <span className={styles.icon} aria-hidden='true'>
      {statusIcon(status)}
    </span>
    <span className={styles.title}>
      <span className={status === 'running' ? shimmerStyles.shimmer : undefined}>{title}</span>
    </span>
    {detail ? (
      <span className={styles.detail} title={detail}>
        {detail}
      </span>
    ) : null}
    {showArrow ? (
      <span className={`${styles.arrow} ${expanded ? styles.arrowExpanded : ''}`} aria-hidden='true'>
        <ChevronRight size={10} strokeWidth={1.75} />
      </span>
    ) : null}
  </>
);

export const TaskRow: React.FC<TaskRowItem & { nested?: boolean }> = (item) => {
  const { id, title, detail, status, onClick, nested } = item;
  const className = `${styles.row} ${statusClass(status)}${nested ? ` ${styles.rowNested}` : ''}`;
  const content = <RowBody title={title} detail={detail} status={status} />;

  return (
    <div className={styles.item} data-testid='beautiful-ui-task-row' data-status={status} data-row-id={id}>
      {onClick ? (
        <button type='button' className={className} onClick={onClick}>
          {content}
        </button>
      ) : (
        <div className={className}>{content}</div>
      )}
      {item.children && item.children.length > 0 ? (
        <div className={styles.children}>
          {item.children.map((child) => (
            <TaskRow key={child.id} {...child} nested />
          ))}
        </div>
      ) : null}
    </div>
  );
};

export const TaskGroup: React.FC<TaskGroupProps> = ({
  title,
  status,
  expandable,
  expanded,
  onToggle,
  ariaControls,
  className,
}) => {
  const resolvedClassName = `${styles.group} ${statusClass(status)}${className ? ` ${className}` : ''}`;
  const content = (
    <RowBody title={title} status={status} showArrow={Boolean(expandable)} expanded={expanded} />
  );

  if (expandable) {
    return (
      <button
        type='button'
        className={resolvedClassName}
        data-testid='beautiful-ui-task-group'
        data-status={status}
        aria-expanded={expanded ?? false}
        aria-controls={ariaControls}
        onClick={onToggle}
      >
        {content}
      </button>
    );
  }

  return (
    <div className={resolvedClassName} data-testid='beautiful-ui-task-group' data-status={status}>
      {content}
    </div>
  );
};

const TaskRows: React.FC<TaskRowsProps> = ({ items, layout = 'list' }) => {
  return (
    <div
      className={`${styles.root} ${layout === 'capsules' ? styles.capsules : styles.list}`}
      data-testid='beautiful-ui-task-rows'
      data-layout={layout}
    >
      {items.map((item, index) => (
        <div key={item.id} className={layout === 'capsules' ? styles.capsule : undefined}>
          {layout === 'capsules' ? <span className={styles.index}>{index + 1}</span> : null}
          <TaskRow {...item} />
        </div>
      ))}
    </div>
  );
};

export default TaskRows;
