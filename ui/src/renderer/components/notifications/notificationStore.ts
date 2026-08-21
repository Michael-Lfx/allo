import type {
  AppNotificationInput,
  AppNotificationUpdate,
  NotificationHandle,
  NotificationScope,
  NotificationScopeOptions,
  StoredNotification,
} from './notificationTypes';
import type { AppNotificationLevel } from './notificationTypes';

const DEFAULT_DURATION = 3000;
const EXIT_DURATION = 120;

type ScopeState = {
  id: string;
  maxCount?: number;
  duration: number;
  closable: boolean;
  disposed: boolean;
};

type Listener = () => void;
type Timer = ReturnType<typeof setTimeout>;

const isFiniteDuration = (value: unknown): value is number => typeof value === 'number' && Number.isFinite(value);

class NotificationStore {
  private records: StoredNotification[] = [];
  private snapshot: readonly StoredNotification[] = [];
  private readonly listeners = new Set<Listener>();
  private readonly scopes = new Map<string, ScopeState>();
  private readonly timers = new Map<string, Timer>();
  private readonly exitTimers = new Map<string, Timer>();
  private readonly timerStartedAt = new Map<string, number>();
  private readonly pauseReasons = new Set<string>();
  private sequence = 0;

  subscribe = (listener: Listener): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  getSnapshot = (): readonly StoredNotification[] => this.snapshot;

  createScope = (options: NotificationScopeOptions = {}): NotificationScope => {
    const id = `notification-scope-${++this.sequence}`;
    const scope: ScopeState = {
      id,
      maxCount: options.maxCount,
      duration: isFiniteDuration(options.duration) ? Math.max(0, options.duration) : DEFAULT_DURATION,
      closable: options.closable ?? false,
      disposed: false,
    };
    this.scopes.set(id, scope);

    return {
      id,
      show: (input) => this.show(scope, input),
      dismiss: (idOrKey) => this.dismiss(scope.id, idOrKey),
      clear: () => this.clear(scope.id),
      configure: (next) => this.configureScope(scope.id, next),
      dispose: () => {
        if (scope.disposed) return;
        scope.disposed = true;
        this.clear(scope.id);
        this.scopes.delete(scope.id);
      },
    };
  };

  pauseInteraction = (reason: string): void => {
    const wasPaused = this.pauseReasons.size > 0;
    this.pauseReasons.add(reason);
    if (!wasPaused) this.pauseTimers();
  };

  resumeInteraction = (reason: string): void => {
    this.pauseReasons.delete(reason);
    if (this.pauseReasons.size === 0) this.resumeTimers();
  };

  show = (scope: ScopeState, input: AppNotificationInput): NotificationHandle => {
    const existing = input.id
      ? this.records.find((record) => record.scopeId === scope.id && record.id === input.id && record.status === 'active')
      : undefined;

    if (existing) {
      this.updateRecord(existing, input, scope, false);
      this.emit();
      return this.createHandle(scope.id, existing.key);
    }

    this.evictIfNeeded(scope);

    const duration = this.resolveDuration(input.duration, scope.duration);
    const record: StoredNotification = {
      key: `notification-${++this.sequence}`,
      scopeId: scope.id,
      id: input.id,
      level: input.level ?? 'info',
      content: input.content,
      title: input.title,
      duration,
      remainingMs: duration,
      closable: input.closable ?? scope.closable,
      showIcon: input.showIcon ?? (input.level ?? 'info') !== 'normal',
      icon: input.icon,
      action: input.action,
      onClose: input.onClose,
      announce: input.announce,
      passthrough: input.passthrough ?? false,
      createdAt: this.sequence,
      revision: 0,
      status: 'active',
    };

    this.records = [...this.records, record];
    this.startTimer(record);
    this.emit();
    return this.createHandle(scope.id, record.key);
  };

  updateByKey = (scopeId: string, key: string, input: AppNotificationUpdate): void => {
    const record = this.records.find((item) => item.scopeId === scopeId && item.key === key && item.status === 'active');
    if (!record) return;
    const scope = this.scopes.get(scopeId);
    if (!scope || scope.disposed) return;

    const duration = input.duration === undefined ? record.duration : this.resolveDuration(input.duration, scope.duration);
    this.updateRecord(
      record,
      {
        ...input,
        content: input.content === undefined ? record.content : input.content,
        duration,
      },
      scope,
      true,
    );
    this.emit();
  };

  dismiss = (scopeId: string, idOrKey: string): void => {
    const record = this.records.find(
      (item) => item.scopeId === scopeId && (item.key === idOrKey || item.id === idOrKey) && item.status === 'active',
    );
    if (record) this.beginExit(record);
  };

  clear = (scopeId?: string): void => {
    const records = this.records.filter((record) => record.status === 'active' && (!scopeId || record.scopeId === scopeId));
    records.forEach((record) => this.beginExit(record));
  };

  private configureScope(scopeId: string, options: NotificationScopeOptions): void {
    const scope = this.scopes.get(scopeId);
    if (!scope) return;
    if (options.maxCount !== undefined) scope.maxCount = options.maxCount;
    if (options.duration !== undefined && isFiniteDuration(options.duration)) {
      scope.duration = Math.max(0, options.duration);
    }
    if (options.closable !== undefined) scope.closable = options.closable;
  }

  private createHandle(scopeId: string, key: string): NotificationHandle {
    return {
      id: key,
      dismiss: () => this.dismiss(scopeId, key),
      update: (input) => this.updateByKey(scopeId, key, input),
    };
  }

  private resolveDuration(value: number | undefined, fallback: number): number {
    if (!isFiniteDuration(value)) return fallback;
    return Math.max(0, value);
  }

  private evictIfNeeded(scope: ScopeState): void {
    if (!scope.maxCount || scope.maxCount < 1) return;
    const active = this.records.filter((record) => record.scopeId === scope.id && record.status === 'active');
    if (active.length < scope.maxCount) return;
    const oldest = active.reduce((result, record) => (record.createdAt < result.createdAt ? record : result));
    this.beginExit(oldest);
  }

  private updateRecord(
    record: StoredNotification,
    input: AppNotificationInput,
    scope: ScopeState,
    preserveDuration: boolean,
  ): void {
    this.clearTimer(record.key);
    const level = input.level ?? record.level;
    const duration = preserveDuration && input.duration === undefined ? record.duration : this.resolveDuration(input.duration, scope.duration);
    record.level = level;
    record.content = input.content;
    record.title = input.title;
    record.duration = duration;
    record.remainingMs = duration;
    record.closable = input.closable ?? record.closable;
    record.showIcon = input.showIcon ?? (level !== 'normal');
    record.icon = input.icon;
    record.action = input.action;
    record.onClose = input.onClose;
    record.announce = input.announce;
    record.passthrough = input.passthrough ?? record.passthrough;
    record.revision += 1;
    this.startTimer(record);
  }

  private startTimer(record: StoredNotification): void {
    if (record.status !== 'active' || record.duration === 0 || this.pauseReasons.size > 0) return;
    this.clearTimer(record.key);
    const remaining = Math.max(0, record.remainingMs);
    if (remaining === 0) {
      this.beginExit(record);
      return;
    }
    this.timerStartedAt.set(record.key, Date.now());
    this.timers.set(
      record.key,
      setTimeout(() => {
        this.clearTimer(record.key);
        this.beginExit(record);
      }, remaining),
    );
  }

  private clearTimer(key: string): void {
    const timer = this.timers.get(key);
    if (timer !== undefined) clearTimeout(timer);
    this.timers.delete(key);
    this.timerStartedAt.delete(key);
  }

  private pauseTimers(): void {
    const now = Date.now();
    this.records.forEach((record) => {
      if (record.status !== 'active') return;
      const startedAt = this.timerStartedAt.get(record.key);
      if (startedAt !== undefined) {
        record.remainingMs = Math.max(0, record.remainingMs - (now - startedAt));
        this.clearTimer(record.key);
      }
    });
  }

  private resumeTimers(): void {
    this.records.forEach((record) => this.startTimer(record));
  }

  private beginExit(record: StoredNotification): void {
    if (record.status !== 'active') return;
    this.clearTimer(record.key);
    record.status = 'exiting';
    this.emit();
    const timer = setTimeout(() => this.finalizeExit(record.key), EXIT_DURATION);
    this.exitTimers.set(record.key, timer);
  }

  private finalizeExit(key: string): void {
    const exitTimer = this.exitTimers.get(key);
    if (exitTimer !== undefined) clearTimeout(exitTimer);
    this.exitTimers.delete(key);
    const record = this.records.find((item) => item.key === key);
    if (!record) return;
    this.records = this.records.filter((item) => item.key !== key);
    this.emit();
    try {
      record.onClose?.();
    } catch (error) {
      console.error('Notification onClose callback failed', error);
    }
  }

  private emit(): void {
    this.snapshot = this.records.slice();
    this.listeners.forEach((listener) => listener());
  }
}

export const notificationStore = new NotificationStore();

export const DEFAULT_NOTIFICATION_DURATION = DEFAULT_DURATION;
export const NOTIFICATION_EXIT_DURATION = EXIT_DURATION;

export type { AppNotificationLevel };
