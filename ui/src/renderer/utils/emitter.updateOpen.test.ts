import { describe, expect, test } from 'bun:test';
import { addEventListener, emitter } from './emitter';
import { normalizeUpdateTelemetrySource } from './analytics/updateTelemetry';

describe('update open event', () => {
  test('delivers sources to each listener and removes only the unsubscribed listener', () => {
    const first: string[] = [];
    const second: string[] = [];
    const unsubscribeFirst = addEventListener('app.update.open', ({ source }) => {
      first.push(normalizeUpdateTelemetrySource(source));
    });
    const unsubscribeSecond = addEventListener('app.update.open', ({ source }) => {
      second.push(normalizeUpdateTelemetrySource(source));
    });

    try {
      emitter.emit('app.update.open', { source: 'titlebar' });
      emitter.emit('app.update.open', { source: 'startup' });
      unsubscribeFirst();
      emitter.emit('app.update.open', { source: 'interval' });
      expect(first).toEqual(['titlebar', 'startup']);
      expect(second).toEqual(['titlebar', 'startup', 'interval']);
    } finally {
      unsubscribeFirst();
      unsubscribeSecond();
    }

    emitter.emit('app.update.open', { source: 'titlebar' });
    expect(second).toEqual(['titlebar', 'startup', 'interval']);
  });
});
