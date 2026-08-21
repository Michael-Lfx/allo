import type { BillingPlanPeriod } from './billingCheckout';

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

export function isUsdItem(item: { currency?: string | null }): boolean {
  const currency = String(item.currency ?? 'USD').trim().toUpperCase();
  return currency === 'USD';
}

export function isFreePlan(plan: BillingPlan): boolean {
  const price = Number(plan.currentPriceCent ?? 0);
  const code = String(plan.code ?? '').trim().toLowerCase();
  return price <= 0 || code === 'free';
}

export function isPurchasablePlan(plan: BillingPlan): boolean {
  return !plan.isCurrent && !isFreePlan(plan) && isUsdItem(plan);
}

export function planPeriodOf(plan: BillingPlan): BillingPlanPeriod | null {
  const period = String(plan.planPeriod ?? '').trim().toUpperCase();
  if (period === 'MONTH' || period === 'HALF_YEAR' || period === 'YEAR') return period;
  return null;
}

export function availablePlanPeriods(plans: BillingPlan[]): BillingPlanPeriod[] {
  const present = new Set(plans.map(planPeriodOf).filter((period): period is BillingPlanPeriod => period !== null));
  return BILLING_PLAN_PERIODS.filter((period) => present.has(period));
}

export function plansForPeriod(plans: BillingPlan[], period: BillingPlanPeriod): BillingPlan[] {
  return plans.filter((plan) => isUsdItem(plan) && planPeriodOf(plan) === period);
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

export function usdPacks(packs: BillingCreditPack[]): BillingCreditPack[] {
  return packs.filter((pack) => isUsdItem(pack) && Number(pack.priceCent ?? 0) > 0);
}
