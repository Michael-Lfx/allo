import { describe, expect, test } from 'bun:test';
import { buildDropInAppearance, buildDropInElementOptions, resolveBillingCheckoutTheme } from './airwallex';

describe('Airwallex embedded checkout options', () => {
  test('matches the drop-in iframe payload', () => {
    expect(
      buildDropInElementOptions({
        intentId: 'int_1',
        clientSecret: 'secret_1',
        currency: 'USD',
        shopperName: 'Ada',
        shopperEmail: 'ada@example.com',
      })
    ).toEqual({
      intent_id: 'int_1',
      client_secret: 'secret_1',
      currency: 'USD',
      methods: ['card'],
      shopper_name: 'Ada',
      shopper_email: 'ada@example.com',
    });
  });

  test('omits empty shopper fields', () => {
    expect(
      buildDropInElementOptions({
        intentId: 'int_2',
        clientSecret: 'secret_2',
        currency: 'USD',
      })
    ).toEqual({
      intent_id: 'int_2',
      client_secret: 'secret_2',
      currency: 'USD',
      methods: ['card'],
    });
  });

  test('skins drop-in to the billing cobalt palette', () => {
    expect(resolveBillingCheckoutTheme('dark')).toBe('dark');
    expect(resolveBillingCheckoutTheme('light')).toBe('light');
    expect(buildDropInAppearance('light').variables.colorBackground).toBe('#ffffff');
    expect(buildDropInAppearance('dark').variables.colorBackground).toBe('#12151c');
  });
});
