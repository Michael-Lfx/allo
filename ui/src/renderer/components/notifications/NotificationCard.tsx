import { Attention, CheckOne, CloseSmall, Info, Loading } from '@icon-park/react';
import React from 'react';
import type { AppNotificationLevel, StoredNotification } from './notificationTypes';

type NotificationCardProps = {
  notice: StoredNotification;
  closeLabel: string;
  onDismiss: () => void;
  cardRef?: React.Ref<HTMLDivElement>;
  style?: React.CSSProperties;
};

const StatusIcon: React.FC<{ level: AppNotificationLevel }> = ({ level }) => {
  const common = { theme: 'outline' as const, size: '18' };
  if (level === 'success') return <CheckOne {...common} aria-hidden='true' />;
  if (level === 'warning' || level === 'error') return <Attention {...common} aria-hidden='true' />;
  if (level === 'loading') return <Loading {...common} className='flowy-notification__loading-icon' aria-hidden='true' />;
  if (level === 'info') return <Info {...common} aria-hidden='true' />;
  return null;
};

const NotificationCard: React.FC<NotificationCardProps> = ({ notice, closeLabel, onDismiss, cardRef, style }) => {
  const showIcon = notice.showIcon && (notice.icon || notice.level !== 'normal');

  return (
    <div
      ref={cardRef}
      style={style}
      className={`flowy-notification-card flowy-notification-card--${notice.level} ${
        notice.status === 'exiting' ? 'flowy-notification-card--exiting' : ''
      } ${notice.passthrough ? 'flowy-notification-card--passthrough' : ''} ${
        notice.duration === 0 ? 'flowy-notification-card--persistent' : ''
      }`}
      data-notification-id={notice.id ?? notice.key}
      data-notification-level={notice.level}
      data-notification-status={notice.status}
    >
      {showIcon && (
        <span key={notice.revision} className='flowy-notification__icon'>
          {notice.icon ?? <StatusIcon level={notice.level} />}
        </span>
      )}
      <div className='flowy-notification__body'>
        {notice.title !== undefined && <div className='flowy-notification__title'>{notice.title}</div>}
        <div className='flowy-notification__content'>{notice.content}</div>
        {notice.action !== undefined && <div className='flowy-notification__action'>{notice.action}</div>}
      </div>
      {notice.closable && notice.status === 'active' && (
        <button type='button' className='flowy-notification__close' aria-label={closeLabel} onClick={onDismiss}>
          <CloseSmall theme='outline' size='16' aria-hidden='true' />
        </button>
      )}
    </div>
  );
};

export default NotificationCard;
