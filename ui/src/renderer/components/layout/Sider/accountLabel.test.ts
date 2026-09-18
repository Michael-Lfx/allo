import { describe, expect, test } from 'bun:test';
import { formatSiderAccountLabel } from './accountLabel';

describe('formatSiderAccountLabel', () => {
  test('prefers a username that differs from the email', () => {
    expect(formatSiderAccountLabel({ username: 'hoyework', email: 'hoyework@qq.com' })).toBe('hoyework');
  });

  test('keeps only the email local part when no username is available', () => {
    expect(formatSiderAccountLabel({ email: 'hoyework@qq.com' })).toBe('hoyework');
  });

  test('does not duplicate an email used as the username', () => {
    expect(formatSiderAccountLabel({ username: 'hoyework@qq.com', email: 'hoyework@qq.com' })).toBe('hoyework');
  });

  test('masks a username that itself looks like an email', () => {
    expect(formatSiderAccountLabel({ username: 'alias@corp.com', email: 'real@home.com' })).toBe('alias');
  });

  test('keeps a handle-like value without a local part untouched', () => {
    expect(formatSiderAccountLabel({ username: '@hoyework' })).toBe('@hoyework');
  });

  test('keeps a value without a domain separator untouched', () => {
    expect(formatSiderAccountLabel({ email: 'hoyework' })).toBe('hoyework');
  });

  test('returns an empty label for an anonymous account', () => {
    expect(formatSiderAccountLabel({})).toBe('');
    expect(formatSiderAccountLabel({ username: '  ', email: null })).toBe('');
  });
});
