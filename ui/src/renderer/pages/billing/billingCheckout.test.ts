import { describe, expect, test } from 'bun:test';
import {
  AIRWALLEX_PAY_CHANNEL,
  buildCreateOrderPayload,
  createCheckoutAttempt,
  decideOrderPoll,
  estimateTotalCents,
  extractAirwallexIntent,
  requireAirwallexChannel,
  retryCheckoutAttempt,
  settlePaidCheckout,
  unwrapChannelList,
} from './billingCheckout';

describe('billing checkout payload', () => {
  test('create-order payload includes airwallex and a stable idempotency key', () => {
    const attempt = createCheckoutAttempt({
      itemType: 'plan',
      itemId: 12,
      name: 'Pro Monthly',
      amountCent: 1990,
      currency: 'USD',
      planPeriod: 'MONTH',
      couponId: 41,
      idempotencyKey: 'attempt-1',
    });

    expect(buildCreateOrderPayload(attempt)).toEqual({
      itemType: 'plan',
      itemId: 12,
      payChannel: AIRWALLEX_PAY_CHANNEL,
      idempotencyKey: 'attempt-1',
      couponId: 41,
      planPeriod: 'MONTH',
    });

    expect(buildCreateOrderPayload(attempt).idempotencyKey).toBe('attempt-1');
    expect(retryCheckoutAttempt(attempt, () => 'attempt-2').idempotencyKey).toBe('attempt-2');
  });

  test('fails closed when airwallex is missing from payment channels', () => {
    expect(requireAirwallexChannel([{ code: 'wechatpay' }])).toBe(false);
    expect(requireAirwallexChannel([{ code: 'airwallex' }, { code: 'wechatpay' }])).toBe(true);
    expect(
      requireAirwallexChannel(
        unwrapChannelList({ list: [{ code: 'airwallex' }, { code: 'wechatpay' }] })
      )
    ).toBe(true);
  });

  test('reads Airwallex intent from nested payment without using the order id', () => {
    expect(
      extractAirwallexIntent({
        id: 99,
        payment: {
          paymentIntentId: 'int_1',
          clientSecret: 'secret_1',
        },
      })
    ).toEqual({ intentId: 'int_1', clientSecret: 'secret_1' });

    expect(extractAirwallexIntent({ id: 'ord_1', clientSecret: 'secret_1' })).toBeNull();
    expect(
      extractAirwallexIntent({ id: 'int_2', clientSecret: 'secret_2' }, { allowIdFallback: true })
    ).toEqual({ intentId: 'int_2', clientSecret: 'secret_2' });
  });
});

describe('billing order polling', () => {
  test('stops on PAID and refreshes credits', async () => {
    const statuses = ['PAYING', 'PAID'];
    let creditsRefreshed = 0;
    let sleeps = 0;

    const decision = await settlePaidCheckout({
      fetchOrder: async () => ({ status: statuses.shift() }),
      refreshCredits: async () => {
        creditsRefreshed += 1;
      },
      sleep: async () => {
        sleeps += 1;
      },
    });

    expect(decision).toBe('paid');
    expect(creditsRefreshed).toBe(1);
    expect(sleeps).toBe(1);
  });

  test('gives up when PAYING never settles and the order has no expiry', async () => {
    let now = 0;
    let fetches = 0;
    const decision = await settlePaidCheckout({
      fetchOrder: async () => {
        fetches += 1;
        return { status: 'PAYING' };
      },
      refreshCredits: async () => {
        throw new Error('credits must not refresh after a poll timeout');
      },
      intervalMs: 10,
      maxWaitMs: 30,
      now: () => now,
      sleep: async (ms) => {
        now += ms;
      },
    });

    expect(decision).toBe('failed');
    expect(fetches).toBeGreaterThan(1);
  });

  test('treats failed, closed, and expired orders as terminal', () => {
    expect(decideOrderPoll({ status: 'FAILED' })).toBe('failed');
    expect(decideOrderPoll({ status: 'CLOSED' })).toBe('failed');
    expect(decideOrderPoll({ status: 'CANCELED' })).toBe('failed');
    expect(decideOrderPoll({ status: 'PAYING', expiresAt: '2020-01-01T00:00:00Z' }, Date.parse('2024-01-01T00:00:00Z'))).toBe(
      'failed'
    );
    expect(decideOrderPoll({ status: 'PAYING' })).toBe('continue');
  });

  test('estimated total never goes below zero', () => {
    expect(estimateTotalCents(1990, 500)).toBe(1490);
    expect(estimateTotalCents(1990, 5000)).toBe(0);
  });
});
