import { addEventListener, emitter } from '@/renderer/utils/emitter';

/** Fired when titlebar search opens a conversation, so the sider list returns to projects. */
export const dispatchSiderHistorySearchSelect = (): void => {
  emitter.emit('sider.history-search.select');
};

/** Keep DOM event semantics: a failing listener must not block the sender or other listeners. */
export const subscribeSiderHistorySearchSelect = (listener: () => void): (() => void) =>
  addEventListener('sider.history-search.select', () => {
    try {
      listener();
    } catch (error) {
      if (typeof globalThis.reportError === 'function') {
        globalThis.reportError(error);
      } else {
        console.error('[historySearchSelect] Listener failed:', error);
      }
    }
  });
