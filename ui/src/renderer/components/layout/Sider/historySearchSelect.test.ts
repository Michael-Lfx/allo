import { describe, expect, test } from 'bun:test';
import { dispatchSiderHistorySearchSelect, subscribeSiderHistorySearchSelect } from './historySearchSelect';

describe('history search selection', () => {
  test('notifies synchronously and stops notifying each unsubscribed listener', () => {
    const calls: string[] = [];
    const unsubscribeFirst = subscribeSiderHistorySearchSelect(() => calls.push('first'));
    const unsubscribeSecond = subscribeSiderHistorySearchSelect(() => calls.push('second'));

    try {
      dispatchSiderHistorySearchSelect();
      expect(calls).toEqual(['first', 'second']);
      unsubscribeFirst();
      dispatchSiderHistorySearchSelect();
      expect(calls).toEqual(['first', 'second', 'second']);
    } finally {
      unsubscribeFirst();
      unsubscribeSecond();
    }

    dispatchSiderHistorySearchSelect();
    expect(calls).toEqual(['first', 'second', 'second']);
  });

  test('reports listener failures without blocking subsequent listeners', () => {
    const originalDescriptor = Object.getOwnPropertyDescriptor(globalThis, 'reportError');
    const failure = new Error('listener failed');
    const errors: unknown[] = [];
    const calls: string[] = [];
    Object.defineProperty(globalThis, 'reportError', { configurable: true, value: (error: unknown) => errors.push(error) });
    const unsubscribeFirst = subscribeSiderHistorySearchSelect(() => { throw failure; });
    const unsubscribeSecond = subscribeSiderHistorySearchSelect(() => calls.push('second'));

    try {
      dispatchSiderHistorySearchSelect();
      expect(errors).toEqual([failure]);
      expect(calls).toEqual(['second']);
    } finally {
      unsubscribeFirst();
      unsubscribeSecond();
      if (originalDescriptor) Object.defineProperty(globalThis, 'reportError', originalDescriptor);
      else Reflect.deleteProperty(globalThis, 'reportError');
    }
  });
});
