import type { NavigateFunction } from 'react-router-dom';
import { BILLING_PATH, type BillingEntryState } from '@renderer/pages/billing/billingRoute';

/**
 * Open the hidden in-app checkout (`#/billing`). The page lists CNY (WeChat Pay)
 * and USD (Airwallex) SKUs, mirroring the official website's overseas checkout.
 * `openOfficialWebsiteCredits` remains available as the website fallback.
 */
export function openInAppBilling(navigate: NavigateFunction, context: BillingEntryState = {}): void {
  const state: BillingEntryState = {
    source: context.source,
    balance: context.balance ?? null,
  };
  navigate(BILLING_PATH, { state });
}
