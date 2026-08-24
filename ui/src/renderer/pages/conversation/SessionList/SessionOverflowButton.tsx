import { Down, Up } from '@icon-park/react';
import classNames from 'classnames';
import React from 'react';
import { useTranslation } from 'react-i18next';

interface SessionOverflowButtonProps {
  expanded: boolean;
  hiddenCount: number;
  controlsId: string;
  onToggle: () => void;
  className?: string;
}

const SessionOverflowButton: React.FC<SessionOverflowButtonProps> = ({
  expanded,
  hiddenCount,
  controlsId,
  onToggle,
  className,
}) => {
  const { t } = useTranslation();
  if (hiddenCount <= 0) return null;

  const label = expanded
    ? t('sessionList.collapseDisplay')
    : t('sessionList.expandDisplay', { count: hiddenCount });

  return (
    <button
      type='button'
      data-testid='session-overflow-button'
      aria-expanded={expanded}
      aria-controls={controlsId}
      className={classNames('session-overflow-button', className)}
      onClick={(event) => {
        event.stopPropagation();
        onToggle();
      }}
    >
      {expanded ? (
        <Up aria-hidden='true' theme='outline' size='13' strokeWidth={3} />
      ) : (
        <Down aria-hidden='true' theme='outline' size='13' strokeWidth={3} />
      )}
      <span className='truncate'>{label}</span>
    </button>
  );
};

export default SessionOverflowButton;
