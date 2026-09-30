import { describe, expect, test } from 'bun:test';
import {
  availablePlanPeriods,
  currentPlanKeys,
  isCurrencyItem,
  isCurrentPlan,
  isPurchasablePlan,
  planActionState,
  localizedPackName,
  localizedPlanName,
  plansForPeriod,
  purchasablePacks,
  type BillingPlan,
} from './billingCatalog';

const plans: BillingPlan[] = [
  { id: 1, code: 'free', planPeriod: 'MONTH', currency: 'USD', currentPriceCent: 0 },
  { id: 2, code: 'plus', planPeriod: 'MONTH', currency: 'USD', currentPriceCent: 1800, name: '进阶版', nameEn: 'Plus' },
  { id: 3, code: 'plus', planPeriod: 'YEAR', currency: 'USD', currentPriceCent: 17280 },
  { id: 4, code: 'plus', planPeriod: 'MONTH', currency: 'CNY', currentPriceCent: 12800 },
  { id: 5, code: 'pro', planPeriod: 'MONTH', currency: 'CNY', currentPriceCent: 25800, isCurrent: true },
];

describe('per-currency catalog', () => {
  test('keeps USD and CNY SKUs apart', () => {
    expect(plansForPeriod(plans, 'MONTH', 'USD').map((plan) => plan.id)).toEqual([1, 2]);
    expect(plansForPeriod(plans, 'MONTH', 'CNY').map((plan) => plan.id)).toEqual([4, 5]);
    expect(plansForPeriod(plans, 'YEAR', 'CNY')).toEqual([]);
  });

  test('treats a missing currency as belonging to the queried catalog', () => {
    expect(isCurrencyItem({}, 'CNY')).toBe(true);
    expect(isCurrencyItem({ currency: 'usd' }, 'USD')).toBe(true);
    expect(isCurrencyItem({ currency: 'USD' }, 'CNY')).toBe(false);
  });

  test('blocks free, current, and wrong-currency plans', () => {
    expect(isPurchasablePlan(plans[0], 'USD')).toBe(false);
    expect(isPurchasablePlan(plans[1], 'USD')).toBe(true);
    expect(isPurchasablePlan(plans[1], 'CNY')).toBe(false);
    expect(isPurchasablePlan(plans[3], 'CNY')).toBe(true);
    expect(isPurchasablePlan(plans[4], 'CNY')).toBe(false);
  });

  test('a plan bought in CNY also shows as current in the USD catalog', () => {
    const usd: BillingPlan[] = [
      { id: 10, code: 'free', planPeriod: 'MONTH', currency: 'USD', currentPriceCent: 0 },
      { id: 11, code: 'pro', planPeriod: 'MONTH', currency: 'USD', currentPriceCent: 1800 },
      { id: 12, code: 'pro', planPeriod: 'YEAR', currency: 'USD', currentPriceCent: 17280 },
      { id: 13, code: 'ultra', planPeriod: 'MONTH', currency: 'USD', currentPriceCent: 3999 },
    ];
    const cny: BillingPlan[] = [
      { id: 20, code: 'free', planPeriod: 'MONTH', currency: 'CNY', currentPriceCent: 0 },
      { id: 21, code: 'Pro', planPeriod: 'MONTH', currency: 'CNY', currentPriceCent: 12600, isCurrent: true },
    ];
    const keys = currentPlanKeys(usd, cny);

    expect(isCurrentPlan(usd[1], keys)).toBe(true);
    expect(isPurchasablePlan(usd[1], 'USD', keys)).toBe(false);
    // Another period of the same plan stays purchasable.
    expect(isCurrentPlan(usd[2], keys)).toBe(false);
    expect(isPurchasablePlan(usd[2], 'USD', keys)).toBe(true);

    expect(planActionState(usd[1], 'USD', keys)).toBe('current');
    expect(planActionState(cny[1], 'CNY', keys)).toBe('current');
    expect(planActionState(usd[3], 'USD', keys)).toBe('select');
  });

  test('Free is labelled free, not current, unless it is the active plan', () => {
    const free: BillingPlan = { id: 1, code: 'free', planPeriod: 'MONTH', currency: 'USD', currentPriceCent: 0 };
    expect(planActionState(free, 'USD', new Set())).toBe('free');
    expect(planActionState({ ...free, isCurrent: true }, 'USD', new Set())).toBe('current');
  });

  test('falls back to the SKU id when a plan has no code', () => {
    const keys = currentPlanKeys([{ id: 7, isCurrent: true }]);
    expect(isCurrentPlan({ id: 7 }, keys)).toBe(true);
    expect(isCurrentPlan({ id: 8 }, keys)).toBe(false);
  });

  test('lists only periods present in the catalog', () => {
    expect(availablePlanPeriods(plans)).toEqual(['MONTH', 'YEAR']);
  });

  test('lists paid packs of the selected currency', () => {
    const packs = [
      { id: 1, currency: 'USD', priceCent: 999 },
      { id: 2, currency: 'CNY', priceCent: 6800 },
      { id: 3, currency: 'CNY', priceCent: 0 },
    ];
    expect(purchasablePacks(packs, 'CNY').map((pack) => pack.id)).toEqual([2]);
    expect(purchasablePacks(packs, 'USD').map((pack) => pack.id)).toEqual([1]);
  });

  test('localizes names with fallbacks', () => {
    expect(localizedPlanName(plans[1], 'zh-CN')).toBe('进阶版');
    expect(localizedPlanName(plans[1], 'en-US')).toBe('Plus');
    expect(localizedPlanName(plans[3], 'en-US')).toBe('plus');
    expect(localizedPackName({ id: 9 }, 'en-US')).toBe('9');
  });
});
