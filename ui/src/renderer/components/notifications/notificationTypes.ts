import type { ReactNode } from 'react';

export type AppNotificationLevel = 'normal' | 'info' | 'success' | 'warning' | 'error' | 'loading';

export type AppNotificationInput = {
  content: ReactNode;
  level?: AppNotificationLevel;
  title?: ReactNode;
  id?: string;
  duration?: number;
  closable?: boolean;
  showIcon?: boolean;
  icon?: ReactNode;
  action?: ReactNode;
  onClose?: () => void;
  announce?: string;
  passthrough?: boolean;
};

export type AppNotificationUpdate = Partial<Omit<AppNotificationInput, 'content'>> & {
  content?: ReactNode;
};

export type NotificationStatus = 'active' | 'exiting';

export type StoredNotification = {
  key: string;
  scopeId: string;
  id?: string;
  level: AppNotificationLevel;
  content: ReactNode;
  title?: ReactNode;
  duration: number;
  remainingMs: number;
  closable: boolean;
  showIcon: boolean;
  icon?: ReactNode;
  action?: ReactNode;
  onClose?: () => void;
  announce?: string;
  passthrough: boolean;
  createdAt: number;
  revision: number;
  status: NotificationStatus;
};

export type NotificationHandle = {
  id: string;
  dismiss: () => void;
  update: (input: AppNotificationUpdate) => void;
};

export type NotificationScopeOptions = {
  maxCount?: number;
  duration?: number;
  closable?: boolean;
};

export type NotificationScope = {
  id: string;
  show: (input: AppNotificationInput) => NotificationHandle;
  dismiss: (id: string) => void;
  clear: () => void;
  configure: (options: NotificationScopeOptions) => void;
  dispose: () => void;
};
