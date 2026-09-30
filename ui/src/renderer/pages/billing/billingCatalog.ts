import type { BillingCurrency, BillingPlanPeriod } from './billingCheckout';

export type BillingPlan = {
  id: number;
  code?: string | null;
  planPeriod?: string | null;
  name?: string | null;
  nameEn?: string | null;
  description?: string | null;
  descriptionEn?: string | null;
  currency?: string | null;
  currentPriceCent?: number | null;
  originalPriceCent?: number | null;
  grantPoints?: number | null;
  durationDays?: number | null;
  durationMonths?: number | null;
  isCurrent?: boolean | null;
  isHot?: boolean | null;
  benefitList?: string[] | null;
  benefitListEn?: string[] | null;
};

export type BillingCreditPack = {
  id: number;
  code?: string | null;
  name?: string | null;
  nameEn?: string | null;
  description?: string | null;
  descriptionEn?: string | null;
  currency?: string | null;
  priceCent?: number | null;
  points?: number | null;
  validDays?: number | null;
};

export const BILLING_PLAN_PERIODS: BillingPlanPeriod[] = ['MONTH', 'HALF_YEAR', 'YEAR'];

/**
 * Items come from a per-currency catalog query; a missing `currency` field means
 * the SKU belongs to the catalog it was listed in. A mismatching one never shows,
 * so a USD SKU can never be paid through WeChat Pay (or CNY through Airwallex).
 */
export function isCurrencyItem(item: { currency?: string | null }, currency: BillingCurrency): boolean {
  const itemCurrency = String(item.currency ?? currency).trim().toUpperCase() || currency;
  return itemCurrency === currency;
}

export function isFreePlan(plan: BillingPlan): boolean {
  const price = Number(plan.currentPriceCent ?? 0);
  const code = String(plan.code ?? '').trim().toLowerCase();
  return price <= 0 || code === 'free';
}

export function planPeriodOf(plan: BillingPlan): BillingPlanPeriod | null {
  const period = String(plan.planPeriod ?? '').trim().toUpperCase();
  if (period === 'MONTH' || period === 'HALF_YEAR' || period === 'YEAR') return period;
  return null;
}

/** Same plan across currencies: USD Pro/month and CNY Pro/month are different SKU ids. */
function planIdentity(plan: BillingPlan): string {
  const code = String(plan.code ?? '').trim().toLowerCase();
  return code ? `${code}|${planPeriodOf(plan) ?? ''}` : `id:${plan.id}`;
}

/**
 * The cloud flags `isCurrent` per SKU, so a subscription bought in CNY only marks
 * the CNY SKU. Collect current identities from every currency catalog so the
 * matching USD plan shows as current too (and vice versa).
 */
export function currentPlanKeys(...catalogs: BillingPlan[][]): Set<string> {
  const keys = new Set<string>();
  for (const plans of catalogs) {
    for (const plan of plans) {
      if (plan.isCurrent) keys.add(planIdentity(plan));
    }
  }
  return keys;
}

export function isCurrentPlan(plan: BillingPlan, currentKeys: Set<string>): boolean {
  return Boolean(plan.isCurrent) || currentKeys.has(planIdentity(plan));
}

export function isPurchasablePlan(
  plan: BillingPlan,
  currency: BillingCurrency,
  currentKeys: Set<string> = new Set()
): boolean {
  return !isCurrentPlan(plan, currentKeys) && !isFreePlan(plan) && isCurrencyItem(plan, currency);
}

export type PlanActionState = 'current' | 'free' | 'select';

/** Button state: only the actual current plan says "current"; Free just can't be bought. */
export function planActionState(
  plan: BillingPlan,
  currency: BillingCurrency,
  currentKeys: Set<string>
): PlanActionState {
  if (isCurrentPlan(plan, currentKeys)) return 'current';
  if (isFreePlan(plan)) return 'free';
  return isPurchasablePlan(plan, currency, currentKeys) ? 'select' : 'free';
}

export function availablePlanPeriods(plans: BillingPlan[]): BillingPlanPeriod[] {
  const present = new Set(plans.map(planPeriodOf).filter((period): period is BillingPlanPeriod => period !== null));
  return BILLING_PLAN_PERIODS.filter((period) => present.has(period));
}

export function plansForPeriod(
  plans: BillingPlan[],
  period: BillingPlanPeriod,
  currency: BillingCurrency
): BillingPlan[] {
  return plans.filter((plan) => isCurrencyItem(plan, currency) && planPeriodOf(plan) === period);
}

export function localizedPlanName(plan: BillingPlan, locale: string): string {
  const useEnglish = !locale.toLowerCase().startsWith('zh');
  const name = useEnglish ? plan.nameEn || plan.name : plan.name || plan.nameEn;
  return String(name || plan.code || '').trim() || String(plan.id);
}

export function localizedPackName(pack: BillingCreditPack, locale: string): string {
  const useEnglish = !locale.toLowerCase().startsWith('zh');
  const name = useEnglish ? pack.nameEn || pack.name : pack.name || pack.nameEn;
  return String(name || pack.code || '').trim() || String(pack.id);
}

export function purchasablePacks(packs: BillingCreditPack[], currency: BillingCurrency): BillingCreditPack[] {
  return packs.filter((pack) => isCurrencyItem(pack, currency) && Number(pack.priceCent ?? 0) > 0);
}
