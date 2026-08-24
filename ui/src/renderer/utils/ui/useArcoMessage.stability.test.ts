import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./useArcoMessage.ts', import.meta.url), 'utf8');
const facadeSource = readFileSync(new URL('../../components/notifications/index.ts', import.meta.url), 'utf8');

describe('useArcoMessage', () => {
  test('sources its API from the unified notifications facade, never from Arco at runtime', () => {
    expect(source.includes("from '@arco-design/web-react'")).toBe(false);
    expect(source.includes("from '@/renderer/components/notifications'")).toBe(true);
    expect(source.includes('AppMessage.useMessage(config)')).toBe(true);
  });

  test('the facade exposes the stable message/notification surface the hook builds on', () => {
    expect(facadeSource.includes('export const appNotifications')).toBe(true);
    expect(facadeSource.includes('show: (input) => globalScope.show(input)')).toBe(true);
    expect(facadeSource.includes('notificationStore.createScope(initialConfigRef.current)')).toBe(true);
    expect(facadeSource.includes('scopeRef.current = notificationStore.createScope(config)')).toBe(false);
    for (const level of ['info', 'success', 'warning', 'error', 'loading', 'normal']) {
      expect(facadeSource.includes(`${level}:`)).toBe(true);
    }
    for (const member of ['clear', 'config', 'useMessage']) {
      expect(facadeSource.includes(`${member}:`)).toBe(true);
    }
    expect(facadeSource.includes('export const AppNotification')).toBe(true);
  });
});
