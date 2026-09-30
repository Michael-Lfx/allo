/**
 * Desktop checkout rules, aligned with the official website's overseas checkout:
 * each catalog currency has exactly one payment channel.
 *   - CNY → WeChat Pay (`wechatpay`, Native QR `codeUrl`)
 *   - USD → Airwallex (`airwallex`, Hosted Payment Page opened in the system browser)
 */
export const AIRWALLEX_PAY_CHANNEL = 'airwallex';
export const WECHAT_PAY_CHANNEL = 'wechatpay';
export const BILLING_POLL_INTERVAL_MS = 2500;
/**
 * Both channels finish outside the page (WeChat scan / browser checkout tab), so
 * polling waits for the user; the order's `expiresAt` usually ends it earlier.
 */
export const BILLING_POLL_MAX_WAIT_MS = 30 * 60_000;

export const BILLING_CURRENCIES = ['USD', 'CNY'] as const;
export type BillingCurrency = (typeof BILLING_CURRENCIES)[number];
export type BillingPayChannel = typeof AIRWALLEX_PAY_CHANNEL | typeof WECHAT_PAY_CHANNEL;
export type BillingItemType = 'plan' | 'pack';
export type BillingPlanPeriod = 'MONTH' | 'HALF_YEAR' | 'YEAR';

export type BillingCreateOrderPayload = {
  itemType: BillingItemType;
  itemId: number;
  payChannel: BillingPayChannel;
  idempotencyKey: string;
  couponId?: number;
  planPeriod?: BillingPlanPeriod;
};

export type BillingCheckoutAttempt = {
  itemType: BillingItemType;
  itemId: number;
  name: string;
  amountCent: number;
  currency: BillingCurrency;
  planPeriod?: BillingPlanPeriod;
  couponId?: number;
  idempotencyKey: string;
};

export type BillingPollDecision = 'continue' | 'paid' | 'failed';

const FAILED_STATUSES = new Set([
  'FAILED',
  'CLOSED',
  'CANCELED',
  'CANCELLED',
  'EXPIRED',
  'REFUNDED',
]);

export function normalizeBillingCurrency(value: unknown, fallback: BillingCurrency = 'USD'): BillingCurrency {
  const normalized = String(value ?? '').trim().toUpperCase();
  return normalized === 'CNY' || normalized === 'USD' ? normalized : fallback;
}

/** The single payment channel used for a catalog currency. */
export function payChannelForCurrency(currency: BillingCurrency): BillingPayChannel {
  return currency === 'CNY' ? WECHAT_PAY_CHANNEL : AIRWALLEX_PAY_CHANNEL;
}

export function newIdempotencyKey(randomUuid: () => string = defaultRandomUuid): string {
  return randomUuid();
}

function defaultRandomUuid(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID();
  }
  return `idem-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

export function createCheckoutAttempt(
  input: Omit<BillingCheckoutAttempt, 'idempotencyKey'> & { idempotencyKey?: string },
  randomUuid: () => string = defaultRandomUuid
): BillingCheckoutAttempt {
  return {
    ...input,
    idempotencyKey: input.idempotencyKey ?? newIdempotencyKey(randomUuid),
  };
}

export function retryCheckoutAttempt(
  attempt: BillingCheckoutAttempt,
  randomUuid: () => string = defaultRandomUuid
): BillingCheckoutAttempt {
  return {
    ...attempt,
    idempotencyKey: newIdempotencyKey(randomUuid),
  };
}

export function buildCreateOrderPayload(attempt: BillingCheckoutAttempt): BillingCreateOrderPayload {
  const payload: BillingCreateOrderPayload = {
    itemType: attempt.itemType,
    itemId: attempt.itemId,
    payChannel: payChannelForCurrency(attempt.currency),
    idempotencyKey: attempt.idempotencyKey,
  };
  if (typeof attempt.couponId === 'number' && attempt.couponId > 0) {
    payload.couponId = attempt.couponId;
  }
  if (attempt.itemType === 'plan' && attempt.planPeriod) {
    payload.planPeriod = attempt.planPeriod;
  }
  return payload;
}

/** Fail closed when the cloud does not offer the currency's channel for this SKU. */
export function hasPayChannel(channels: Array<{ code?: string | null }>, channel: BillingPayChannel): boolean {
  return channels.some((item) => String(item.code ?? '').trim().toLowerCase() === channel);
}

export function unwrapChannelList(
  payload: Array<{ code?: string | null }> | { list?: Array<{ code?: string | null }> | null } | null | undefined
): Array<{ code?: string | null }> {
  if (Array.isArray(payload)) return payload;
  if (payload && Array.isArray(payload.list)) return payload.list;
  return [];
}

export type AirwallexIntentSource = {
  paymentIntentId?: string | null;
  intentId?: string | null;
  id?: string | number | null;
  clientSecret?: string | null;
  client_secret?: string | null;
  payment?: AirwallexIntentSource | null;
};

export function extractAirwallexIntent(
  source: AirwallexIntentSource | null | undefined,
  options?: { allowIdFallback?: boolean }
): { intentId: string; clientSecret: string } | null {
  if (!source) return null;
  const nested = source.payment && typeof source.payment === 'object' ? source.payment : null;
  const intentId = String(
    source.paymentIntentId ||
      source.intentId ||
      nested?.paymentIntentId ||
      nested?.intentId ||
      (options?.allowIdFallback ? nested?.id || source.id : nested?.id) ||
      ''
  ).trim();
  const clientSecret = String(
    source.clientSecret || source.client_secret || nested?.clientSecret || nested?.client_secret || ''
  ).trim();
  if (!intentId || !clientSecret) return null;
  return { intentId, clientSecret };
}

export type WechatCodeUrlSource = {
  codeUrl?: string | null;
  code_url?: string | null;
  payCodeUrl?: string | null;
  payment?: WechatCodeUrlSource | null;
};

/** WeChat Native QR payload from a create-order, order detail, or `/pay` response. */
export function extractWechatCodeUrl(source: WechatCodeUrlSource | null | undefined): string {
  if (!source) return '';
  const nested = source.payment && typeof source.payment === 'object' ? source.payment : null;
  return String(
    nested?.codeUrl || nested?.code_url || source.codeUrl || source.code_url || source.payCodeUrl || ''
  ).trim();
}

export function decideOrderPoll(
  order: { status?: string | null; expiresAt?: string | null },
  now = Date.now()
): BillingPollDecision {
  const status = String(order.status ?? '').trim().toUpperCase();
  if (status === 'PAID') return 'paid';
  if (FAILED_STATUSES.has(status)) return 'failed';
  if (order.expiresAt) {
    const expiresAt = Date.parse(order.expiresAt);
    if (Number.isFinite(expiresAt) && expiresAt <= now) return 'failed';
  }
  return 'continue';
}

export async function pollOrderUntilSettled(options: {
  fetchOrder: () => Promise<{ status?: string | null; expiresAt?: string | null }>;
  intervalMs?: number;
  maxWaitMs?: number;
  sleep?: (ms: number) => Promise<void>;
  now?: () => number;
  isAborted?: () => boolean;
}): Promise<BillingPollDecision> {
  const sleep = options.sleep ?? ((ms: number) => new Promise((resolve) => setTimeout(resolve, ms)));
  const intervalMs = options.intervalMs ?? BILLING_POLL_INTERVAL_MS;
  const maxWaitMs = options.maxWaitMs ?? BILLING_POLL_MAX_WAIT_MS;
  const startedAt = options.now?.() ?? Date.now();

  while (true) {
    if (options.isAborted?.()) return 'failed';
    const order = await options.fetchOrder();
    const now = options.now?.() ?? Date.now();
    const decision = decideOrderPoll(order, now);
    if (decision !== 'continue') return decision;
    if (now - startedAt >= maxWaitMs) return 'failed';
    await sleep(intervalMs);
  }
}

export async function settlePaidCheckout(options: {
  fetchOrder: () => Promise<{ status?: string | null; expiresAt?: string | null }>;
  refreshCredits: () => Promise<void>;
  intervalMs?: number;
  maxWaitMs?: number;
  sleep?: (ms: number) => Promise<void>;
  now?: () => number;
  isAborted?: () => boolean;
}): Promise<BillingPollDecision> {
  const decision = await pollOrderUntilSettled(options);
  if (decision === 'paid') {
    await options.refreshCredits();
  }
  return decision;
}

export function estimateTotalCents(amountCent: number, discountCent = 0): number {
  return Math.max(0, Math.round(amountCent) - Math.max(0, Math.round(discountCent)));
}

/** `$18.00` / `¥128.00`; CNY uses the narrow `¥` symbol even in English (not `CN¥`). */
export function formatMoneyFromCents(cents: number, currency: BillingCurrency, locale: string): string {
  const language = locale.toLowerCase().startsWith('zh') ? 'zh-CN' : 'en-US';
  return new Intl.NumberFormat(language, {
    style: 'currency',
    currency,
    currencyDisplay: currency === 'CNY' ? 'narrowSymbol' : 'symbol',
  }).format(cents / 100);
}

export function couponAppliesToItem(
  coupon: { applicableItemTypes?: string | null; currency?: string | null; status?: string | null },
  itemType: BillingItemType,
  currency: BillingCurrency = 'USD'
): boolean {
  if (String(coupon.status ?? 'UNUSED').toUpperCase() !== 'UNUSED') return false;
  const couponCurrency = String(coupon.currency ?? currency).trim().toUpperCase();
  if (couponCurrency && couponCurrency !== currency) return false;
  const applicable = String(coupon.applicableItemTypes ?? 'all')
    .split(',')
    .map((part) => part.trim().toLowerCase())
    .filter(Boolean);
  if (applicable.length === 0 || applicable.includes('all')) return true;
  return applicable.includes(itemType);
}
