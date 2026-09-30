import { describe, expect, test } from 'bun:test';
import {
  buildCreateOrderPayload,
  couponAppliesToItem,
  createCheckoutAttempt,
  decideOrderPoll,
  estimateTotalCents,
  extractAirwallexIntent,
  extractWechatCodeUrl,
  formatMoneyFromCents,
  hasPayChannel,
  normalizeBillingCurrency,
  payChannelForCurrency,
  retryCheckoutAttempt,
  settlePaidCheckout,
  unwrapChannelList,
} from './billingCheckout';

const uuid = (() => {
  let n = 0;
  return () => `key-${++n}`;
})();

describe('currency → payment channel (same as the website overseas checkout)', () => {
  test('CNY pays with WeChat Pay and USD pays with Airwallex', () => {
    expect(payChannelForCurrency('CNY')).toBe('wechatpay');
    expect(payChannelForCurrency('USD')).toBe('airwallex');
  });

  test('normalizes unknown currencies to the fallback', () => {
    expect(normalizeBillingCurrency(' cny ')).toBe('CNY');
    expect(normalizeBillingCurrency('usd')).toBe('USD');
    expect(normalizeBillingCurrency('JPY')).toBe('USD');
    expect(normalizeBillingCurrency(null, 'CNY')).toBe('CNY');
  });

  test('create-order payload picks the channel from the SKU currency', () => {
    const cny = createCheckoutAttempt(
      { itemType: 'plan', itemId: 21, name: 'Plus', amountCent: 12800, currency: 'CNY', planPeriod: 'MONTH', couponId: 3 },
      uuid
    );
    expect(buildCreateOrderPayload(cny)).toEqual({
      itemType: 'plan',
      itemId: 21,
      payChannel: 'wechatpay',
      idempotencyKey: cny.idempotencyKey,
      couponId: 3,
      planPeriod: 'MONTH',
    });

    const usd = createCheckoutAttempt(
      { itemType: 'pack', itemId: 11, name: '10k', amountCent: 999, currency: 'USD', planPeriod: 'YEAR' },
      uuid
    );
    expect(buildCreateOrderPayload(usd)).toEqual({
      itemType: 'pack',
      itemId: 11,
      payChannel: 'airwallex',
      idempotencyKey: usd.idempotencyKey,
    });
  });

  test('keeps the idempotency key for one attempt and rotates it on retry', () => {
    const attempt = createCheckoutAttempt(
      { itemType: 'pack', itemId: 1, name: 'p', amountCent: 100, currency: 'USD' },
      uuid
    );
    expect(buildCreateOrderPayload(attempt).idempotencyKey).toBe(attempt.idempotencyKey);
    expect(retryCheckoutAttempt(attempt, uuid).idempotencyKey).not.toBe(attempt.idempotencyKey);
  });

  test('fails closed unless the cloud offers the expected channel', () => {
    const channels = unwrapChannelList({ list: [{ code: 'WeChatPay' }] });
    expect(hasPayChannel(channels, 'wechatpay')).toBe(true);
    expect(hasPayChannel(channels, 'airwallex')).toBe(false);
    expect(hasPayChannel(unwrapChannelList(null), 'airwallex')).toBe(false);
  });
});

describe('payment entry extraction', () => {
  test('reads the WeChat QR from create-order, /pay, or legacy order rows', () => {
    expect(extractWechatCodeUrl({ payment: { codeUrl: 'weixin://a' } })).toBe('weixin://a');
    expect(extractWechatCodeUrl({ codeUrl: ' weixin://b ' })).toBe('weixin://b');
    expect(extractWechatCodeUrl({ payCodeUrl: 'weixin://c' })).toBe('weixin://c');
    expect(extractWechatCodeUrl({ payment: {} })).toBe('');
    expect(extractWechatCodeUrl(null)).toBe('');
  });

  test('reads the Airwallex intent from the order payment block', () => {
    expect(extractAirwallexIntent({ payment: { paymentIntentId: 'int_1', clientSecret: 's_1' } })).toEqual({
      intentId: 'int_1',
      clientSecret: 's_1',
    });
    expect(extractAirwallexIntent({ payment: { codeUrl: 'weixin://a' } as never })).toBeNull();
  });
});

describe('order polling', () => {
  test('settles on PAID, failed statuses, and expiry', () => {
    expect(decideOrderPoll({ status: 'paid' })).toBe('paid');
    expect(decideOrderPoll({ status: 'CLOSED' })).toBe('failed');
    expect(decideOrderPoll({ status: 'CREATED', expiresAt: '2026-01-01T00:00:00Z' }, Date.parse('2026-01-02'))).toBe(
      'failed'
    );
    expect(decideOrderPoll({ status: 'PAYING' })).toBe('continue');
  });

  test('refreshes credits only after the order is paid', async () => {
    const statuses = ['CREATED', 'PAYING', 'PAID'];
    let refreshed = 0;
    const decision = await settlePaidCheckout({
      fetchOrder: async () => ({ status: statuses.shift() }),
      refreshCredits: async () => {
        refreshed += 1;
      },
      sleep: async () => undefined,
    });
    expect(decision).toBe('paid');
    expect(refreshed).toBe(1);
  });

  test('stops when the wait window elapses', async () => {
    let now = 0;
    const decision = await settlePaidCheckout({
      fetchOrder: async () => ({ status: 'CREATED' }),
      refreshCredits: async () => {
        throw new Error('must not refresh');
      },
      maxWaitMs: 5000,
      now: () => now,
      sleep: async (ms) => {
        now += ms;
      },
    });
    expect(decision).toBe('failed');
  });
});

describe('amounts and coupons', () => {
  test('formats USD with $ and CNY with ¥ in both locales', () => {
    expect(formatMoneyFromCents(1800, 'USD', 'en-US')).toBe('$18.00');
    expect(formatMoneyFromCents(12800, 'CNY', 'en-US')).toBe('¥128.00');
    expect(formatMoneyFromCents(12800, 'CNY', 'zh-CN')).toBe('¥128.00');
  });

  test('never discounts below zero', () => {
    expect(estimateTotalCents(1000, 300)).toBe(700);
    expect(estimateTotalCents(1000, 5000)).toBe(0);
  });

  test('only offers coupons in the checkout currency', () => {
    expect(couponAppliesToItem({ currency: 'CNY', applicableItemTypes: 'plan' }, 'plan', 'CNY')).toBe(true);
    expect(couponAppliesToItem({ currency: 'USD' }, 'plan', 'CNY')).toBe(false);
    expect(couponAppliesToItem({ currency: 'CNY', applicableItemTypes: 'pack' }, 'plan', 'CNY')).toBe(false);
    expect(couponAppliesToItem({ status: 'USED' }, 'plan', 'USD')).toBe(false);
  });
});
