import { useEffect, useRef } from 'react';
import type { ReactNode } from 'react';
import NotificationHost from './NotificationHost';
import { notificationStore } from './notificationStore';
import type {
  AppNotificationInput,
  AppNotificationLevel,
  AppNotificationUpdate,
  NotificationHandle,
  NotificationScope,
  NotificationScopeOptions,
} from './notificationTypes';

export type { AppNotificationInput, AppNotificationLevel, AppNotificationUpdate, NotificationHandle } from './notificationTypes';
export { mergeRefs, useNotificationBlocker } from './notificationInsets';
export { notificationStore } from './notificationStore';

type MessageScope = NotificationScope;
type ScopeAccessor = () => MessageScope | null;

export type AppMessageConfig = Omit<AppNotificationInput, 'level' | 'content'> & { content: ReactNode };
export type AppMessageMethod = (config: AppMessageConfig | string) => () => void;
export type AppMessageInstance = {
  info: AppMessageMethod;
  success: AppMessageMethod;
  warning: AppMessageMethod;
  error: AppMessageMethod;
  loading: AppMessageMethod;
  normal: AppMessageMethod;
};

export type AppMessageUseMessageConfig = NotificationScopeOptions;
export type AppMessageUseMessageReturn = [AppMessageInstance, null];

const globalScope = notificationStore.createScope({ closable: false, duration: 3000 });

const showMessage = (getScope: ScopeAccessor, level: AppNotificationLevel, config: AppMessageConfig | string) => {
  const scope = getScope();
  if (!scope) return () => undefined;
  const input = typeof config === 'string' ? { content: config } : config;
  return scope.show({ ...input, level }).dismiss;
};

const createMessageApi = (getScope: ScopeAccessor): AppMessageInstance => ({
  info: (config) => showMessage(getScope, 'info', config),
  success: (config) => showMessage(getScope, 'success', config),
  warning: (config) => showMessage(getScope, 'warning', config),
  error: (config) => showMessage(getScope, 'error', config),
  loading: (config) => showMessage(getScope, 'loading', config),
  normal: (config) => showMessage(getScope, 'normal', config),
});

export function useNotifications(config?: AppMessageUseMessageConfig): AppMessageUseMessageReturn {
  const scopeRef = useRef<MessageScope | null>(null);
  const initialConfigRef = useRef(config);
  const apiRef = useRef<AppMessageInstance | null>(null);
  if (!apiRef.current) apiRef.current = createMessageApi(() => scopeRef.current);

  useEffect(() => {
    const scope = notificationStore.createScope(initialConfigRef.current);
    scopeRef.current = scope;
    return () => {
      if (scopeRef.current === scope) scopeRef.current = null;
      scope.dispose();
    };
  }, []);

  return [apiRef.current, null];
}

export type AppNotificationsApi = {
  show: (input: AppNotificationInput) => NotificationHandle;
  clear: () => void;
};

export const appNotifications: AppNotificationsApi = {
  show: (input) => globalScope.show(input),
  clear: () => globalScope.clear(),
};

const messageMethod = (level: AppNotificationLevel): AppMessageMethod => (config) =>
  showMessage(() => globalScope, level, config);

export const AppMessage = {
  info: messageMethod('info'),
  success: messageMethod('success'),
  warning: messageMethod('warning'),
  error: messageMethod('error'),
  loading: messageMethod('loading'),
  normal: messageMethod('normal'),
  clear: () => globalScope.clear(),
  config: (config: NotificationScopeOptions) => globalScope.configure(config),
  useMessage: (config?: AppMessageUseMessageConfig): AppMessageUseMessageReturn => useNotifications(config),
};

type AppNotificationConfig = Omit<AppNotificationInput, 'level' | 'content'> & { content: ReactNode };
type AppNotificationMethod = (config: AppNotificationConfig | string) => () => void;

const showNotification = (level: AppNotificationLevel, config: AppNotificationConfig | string): (() => void) => {
  const input = typeof config === 'string' ? { content: config } : config;
  return appNotifications.show({ ...input, level, closable: input.closable ?? true }).dismiss;
};

export const AppNotification: Record<AppNotificationLevel, AppNotificationMethod> & {
  clear: () => void;
} = {
  info: (config) => showNotification('info', config),
  success: (config) => showNotification('success', config),
  warning: (config) => showNotification('warning', config),
  error: (config) => showNotification('error', config),
  loading: (config) => showNotification('loading', config),
  normal: (config) => showNotification('normal', config),
  clear: () => globalScope.clear(),
};

export { NotificationHost };
