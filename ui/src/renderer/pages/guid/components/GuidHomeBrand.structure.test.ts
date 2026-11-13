import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

describe('Guid homepage membership card', () => {
  test('uses the original FlipCard motion with a full-month check-in front', () => {
    const source = readFileSync(new URL('./GuidHomeBrand.tsx', import.meta.url), 'utf8');

    expect(source.includes('FlipCard')).toBe(true);
    expect(source.includes('interactive={false}')).toBe(true);
    expect(source.includes('tilt')).toBe(true);
    expect(source.includes('glare')).toBe(true);
    expect(source.includes('hoverScale')).toBe(true);
    expect(source.includes('getCheckinRecords')).toBe(true);
    expect(source.includes('calendarDays')).toBe(true);
    expect(source.includes('PlusField')).toBe(false);
    expect(source.includes('styles.stars')).toBe(false);
    expect(source.includes("data-testid='guid-home-brand'")).toBe(true);
    expect(source.includes("data-testid='guid-home-calendar'")).toBe(true);
    expect(source.includes("data-testid='guid-home-checkin-status'")).toBe(true);
    expect(source.includes("data-testid='guid-home-credits-detail'")).toBe(true);
    expect(source.includes("shadowColor='var(--flowy-accent)'")).toBe(true);
    expect(source.includes('#7f93b8')).toBe(false);
  });

  test('keeps the compact back as a message list with a real detail button', () => {
    const source = readFileSync(new URL('./GuidHomeBrand.tsx', import.meta.url), 'utf8');

    expect(source.includes('cardMessages')).toBe(true);
    expect(source.includes('cardAvailableCredits')).toBe(true);
    expect(source.includes('cardMonthCredits')).toBe(true);
    expect(source.includes('cardDaysUnit')).toBe(true);
    expect(source.includes('cardViewDetails')).toBe(true);
    expect(source.includes('openInAppBilling')).toBe(true);
    expect(source.includes('cardQuoteCool')).toBe(false);
    expect(source.includes('announcement')).toBe(false);
    expect(source.includes('公告')).toBe(false);
  });

  test('keeps the theme wash and neutral check-in tokens without fixed decoration colors', () => {
    const css = readFileSync(new URL('./GuidHomeCard.module.css', import.meta.url), 'utf8');

    expect(css.includes('--card-accent: var(--brand')).toBe(true);
    expect(css.includes('var(--card-accent-wash)')).toBe(true);
    expect(css.includes('--card-base-end')).toBe(true);
    expect(css.includes('--card-signed-bg')).toBe(true);
    expect(css.includes('--card-today-ring')).toBe(true);
    expect(css.includes('rgb(var(--primary-6))')).toBe(false);
    expect(css.includes('.stars')).toBe(false);
    expect(css.includes('stroke: color-mix(in srgb, var(--card-accent) 45%, var(--card-muted))')).toBe(true);
  });
});
