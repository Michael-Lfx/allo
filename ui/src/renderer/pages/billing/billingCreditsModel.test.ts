import { describe, expect, test } from 'bun:test';
import { calendarDays, expiringBuckets, recordDate, zonedDayKey } from './billingCreditsModel';

describe('credit history view model', () => {
  test('uses Monday-first fixed-size calendars, including leap months', () => {
    const october = calendarDays('2026-10');
    expect(october.length).toBe(42);
    expect(october.slice(0, 4)).toEqual([null, null, null, '2026-10-01']);
    expect(october.filter(Boolean).length).toBe(31);
    expect(calendarDays('2024-02').filter(Boolean).length).toBe(29);
  });
  test('server day key wins over timestamps that fall on the previous UTC day', () => {
    expect(recordDate({ dayKey: 20261012, date: '2026-10-11' })).toBe('2026-10-12');
  });
  test('resolves the check-in day from server time in the check-in zone, not UTC', () => {
    expect(zonedDayKey('2026-10-31T16:30:00Z', 'Asia/Shanghai')).toBe('2026-11-01');
    expect(zonedDayKey('2026-11-01T01:00:00Z', 'Asia/Shanghai')).toBe('2026-11-01');
    expect(zonedDayKey('2026-10-31T23:30:00-08:00', 'Asia/Shanghai')).toBe('2026-11-01');
    expect(zonedDayKey('2026-10-12T00:00:00Z', 'UTC')).toBe('2026-10-12');
    expect(zonedDayKey('not-a-date', 'Asia/Shanghai')).toBeNull();
    expect(zonedDayKey('2026-11-01T00:00:00Z', 'Not/AZone')).toBeNull();
  });
  test('sorts only positive active batches using server time', () => {
    const bucket = (expireAt: string | null, remaining: number) => ({ expireAt, remaining, total: 100, used: 0 });
    const actual = expiringBuckets({ serverTime: '2026-10-12T00:00:00Z', balance: 100, includeTeamSeat: true, list: [{ type: 'PACK', total: 100, used: 0, remaining: 100, buckets: [bucket('2027-01-01', 10), bucket('2026-10-01', 10), bucket(null, 10), bucket('2026-10-13', 20), bucket('2026-10-14', 0)] }] });
    expect(actual.map((row) => row.remaining)).toEqual([20, 10]);
  });
});
