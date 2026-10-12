import type { IMediaCreditsCheckinRecord, IMediaCreditsUsageByType } from '@/common/adapter/ipcBridge';

export function monthKey(date: Date): string {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}`;
}

/**
 * `YYYY-MM-DD` of an instant in the given IANA time zone, or null when the
 * timestamp cannot be parsed or the zone is invalid. The check-in day belongs
 * to the posted check-in zone, so UTC slicing or the browser clock must never
 * be used to derive "today".
 */
export function zonedDayKey(value: string | Date, timeZone: string): string | null {
  const instant = value instanceof Date ? value : new Date(value);
  if (Number.isNaN(instant.getTime())) return null;
  try {
    const parts = new Intl.DateTimeFormat('en-CA', {
      timeZone,
      year: 'numeric',
      month: '2-digit',
      day: '2-digit',
    }).formatToParts(instant);
    const part = (type: 'year' | 'month' | 'day') => parts.find((item) => item.type === type)?.value ?? '';
    const key = `${part('year')}-${part('month')}-${part('day')}`;
    return /^\d{4}-\d{2}-\d{2}$/.test(key) ? key : null;
  } catch {
    return null;
  }
}

export function calendarDays(month: string): (string | null)[] {
  const [year, index] = month.split('-').map(Number);
  const offset = (new Date(year, index - 1, 1).getDay() + 6) % 7;
  const count = new Date(year, index, 0).getDate();
  return Array.from({ length: 42 }, (_, i) => i >= offset && i < offset + count
    ? `${month}-${String(i - offset + 1).padStart(2, '0')}` : null);
}

export function recordDate(record: Pick<IMediaCreditsCheckinRecord, 'dayKey' | 'date'>): string {
  const key = String(record.dayKey);
  return /^\d{8}$/.test(key) ? `${key.slice(0, 4)}-${key.slice(4, 6)}-${key.slice(6)}` : record.date;
}

export function expiringBuckets(usage: IMediaCreditsUsageByType) {
  // A malformed server clock must not silently hide every batch; fall back to
  // the local clock so valid future expiry dates still surface.
  const serverNow = Date.parse(usage.serverTime);
  const cutoff = Number.isFinite(serverNow) ? serverNow : Date.now();
  return usage.list.flatMap((source) => source.buckets
    .filter((bucket) => bucket.remaining > 0 && bucket.expireAt &&
      Number.isFinite(Date.parse(bucket.expireAt)) && Date.parse(bucket.expireAt) > cutoff)
    .map((bucket) => ({ ...bucket, type: source.type })))
    .sort((a, b) => Date.parse(a.expireAt!) - Date.parse(b.expireAt!));
}

/**
 * Track automatic values (today's month/date) only while the user has not
 * chosen something else: `current` equal to the previous automatic value is
 * replaced by `next`, any manual divergence is preserved.
 */
export function adoptAutomaticValue(previous: string, next: string, current: string): string {
  return current === previous ? next : current;
}
