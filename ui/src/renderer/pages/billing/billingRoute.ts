import { normalizeBillingCurrency, type BillingCurrency } from './billingCheckout';

/** Hidden in-app checkout route (not listed in any nav surface). */
export const BILLING_PATH = '/billing';
export const BILLING_CURRENCY_STORAGE_KEY = 'flowy.billing.currency';
/** Same default as the official website's overseas pricing page. */
export const DEFAULT_BILLING_CURRENCY: BillingCurrency = 'USD';

export type BillingEntrySource =
  | 'sider'
  | 'sendbox'
  | 'conversation_error_card'
  | 'video_failure_card'
  | 'video_launch'
  | 'canvas_credits';

export type BillingEntryState = {
  source?: BillingEntrySource;
  balance?: number | null;
};

type BillingStore = Pick<Storage, 'getItem' | 'setItem'>;

function defaultStore(): BillingStore | null {
  try {
    return typeof sessionStorage === 'undefined' ? null : sessionStorage;
  } catch {
    return null;
  }
}

export function readBillingCurrency(store: BillingStore | null = defaultStore()): BillingCurrency {
  try {
    return normalizeBillingCurrency(store?.getItem(BILLING_CURRENCY_STORAGE_KEY), DEFAULT_BILLING_CURRENCY);
  } catch {
    return DEFAULT_BILLING_CURRENCY;
  }
}

export function writeBillingCurrency(
  currency: BillingCurrency,
  store: BillingStore | null = defaultStore()
): BillingCurrency {
  try {
    store?.setItem(BILLING_CURRENCY_STORAGE_KEY, currency);
  } catch {
    // Storage can be blocked; the in-memory selection still works.
  }
  return currency;
}

export function readBillingEntryState(state: unknown): BillingEntryState {
  if (!state || typeof state !== 'object') return {};
  const value = state as BillingEntryState;
  return {
    source: value.source,
    balance: typeof value.balance === 'number' ? value.balance : null,
  };
}
