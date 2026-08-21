import { describe, expect, test } from 'bun:test';
import {
  availablePlanPeriods,
  isPurchasablePlan,
  localizedPlanName,
  plansForPeriod,
  usdPacks,
} from './billingCatalog';

describe('billing catalog', () => {
  test('marks current and free plans as non-purchasable', () => {
    expect(
      isPurchasablePlan({
        id: 1,
        currency: 'USD',
        currentPriceCent: 1990,
        isCurrent: true,
      })
    ).toBe(false);
    expect(
      isPurchasablePlan({
        id: 2,
        code: 'free',
        currency: 'USD',
        currentPriceCent: 0,
      })
    ).toBe(false);
    expect(
      isPurchasablePlan({
        id: 3,
        currency: 'USD',
        currentPriceCent: 1990,
        isCurrent: false,
      })
    ).toBe(true);
  });

  test('filters USD rows by billing period', () => {
    const plans = [
      { id: 1, planPeriod: 'MONTH', currency: 'USD', currentPriceCent: 1000 },
      { id: 2, planPeriod: 'YEAR', currency: 'USD', currentPriceCent: 10000 },
      { id: 3, planPeriod: 'MONTH', currency: 'CNY', currentPriceCent: 1990 },
    ];
    expect(availablePlanPeriods(plans)).toEqual(['MONTH', 'YEAR']);
    expect(plansForPeriod(plans, 'MONTH').map((plan) => plan.id)).toEqual([1]);
  });

  test('prefers English names for en locales', () => {
    expect(localizedPlanName({ id: 1, name: '专业月卡', nameEn: 'Pro Monthly' }, 'en-US')).toBe('Pro Monthly');
    expect(localizedPlanName({ id: 1, name: '专业月卡', nameEn: 'Pro Monthly' }, 'zh-CN')).toBe('专业月卡');
    expect(usdPacks([{ id: 1, currency: 'USD', priceCent: 990 }, { id: 2, currency: 'CNY', priceCent: 990 }]).map((pack) => pack.id)).toEqual([
      1,
    ]);
  });
});
