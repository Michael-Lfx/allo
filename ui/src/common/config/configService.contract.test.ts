import { describe, expect, test } from 'bun:test';
import { getChangedConfigKeys } from './configService';

describe('configService reload diff', () => {
  test('reports changed and deleted keys while ignoring equivalent snapshots', () => {
    const previous = new Map<string, unknown>([
      ['language', 'zh-CN'],
      ['theme', 'light'],
      ['customCss', '.old {}'],
    ]);
    const next = new Map<string, unknown>([
      ['language', 'en-US'],
      ['theme', 'light'],
      ['css.themes', [{ id: 'dark', css: 'body {}' }]],
    ]);

    expect(getChangedConfigKeys(previous, next)).toEqual(['language', 'customCss', 'css.themes']);
  });

  test('compares structured values by content rather than object identity', () => {
    const previous = new Map<string, unknown>([['window.bounds', { width: 1200, height: 800 }]]);
    const next = new Map<string, unknown>([['window.bounds', { width: 1200, height: 800 }]]);

    expect(getChangedConfigKeys(previous, next)).toEqual([]);
  });
});
