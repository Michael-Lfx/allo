import { useEffect, useRef } from 'react';
import type { ReactNode } from 'react';
import NotificationHost from './NotificationHost';
import { notificationStore } from './notificationStore';
import type {
  AppNotificationInput,
  AppNotificationLevel,
  AppNotificationUpdate,
  NotificationHandle,
  NotificationScopeOptions,
} from './notificationTypes';

export type { AppNotificationInput, AppNotificationLevel, AppNotificationUpdate, NotificationHandle } from './notificationTypes';
export { mergeRefs, useNotificationBlocker } from './notificationInsets';
export { notificationStore } from './notificationStore';

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

const showMessage = (scope: ReturnType<typeof notificationStore.createScope>, level: AppNotificationLevel, config: AppMessageConfig | string) => {
  const input = typeof config === 'string' ? { content: config } : config;
  return scope.show({ ...input, level }).dismiss;
};

const createMessageApi = (scope: ReturnType<typeof notificationStore.createScope>): AppMessageInstance => ({
  info: (config) => showMessage(scope, 'info', config),
  success: (config) => showMessage(scope, 'success', config),
  warning: (config) => showMessage(scope, 'warning', config),
  error: (config) => showMessage(scope, 'error', config),
  loading: (config) => showMessage(scope, 'loading', config),
  normal: (config) => showMessage(scope, 'normal', config),
});

export function useNotifications(config?: AppMessageUseMessageConfig): AppMessageUseMessageReturn {
  const scopeRef = useRef<ReturnType<typeof notificationStore.createScope> | null>(null);
  const apiRef = useRef<AppMessageInstance | null>(null);
  if (!scopeRef.current) scopeRef.current = notificationStore.createScope(config);
  if (!apiRef.current) apiRef.current = createMessageApi(scopeRef.current);
  const scope = scopeRef.current;
  useEffect(() => () => scope.dispose(), [scope]);
  return [apiRef.current, null];
}

const messageMethod = (level: AppNotificationLevel): AppMessageMethod => (config) => showMessage(globalScope, level, config);

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
  return globalScope.show({ ...input, level, closable: input.closable ?? true }).dismiss;
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
