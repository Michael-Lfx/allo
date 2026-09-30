import { describe, expect, test } from 'bun:test';
import { buildHostedCheckoutOptions, isAirwallexCheckoutUrl, resolveAirwallexEnv } from './airwallex';

describe('Airwallex hosted checkout (same page as the website)', () => {
  test('asks the SDK for the URL instead of navigating the app window', () => {
    expect(
      buildHostedCheckoutOptions({
        intentId: 'int_1',
        clientSecret: 'secret_1',
        currency: 'USD',
        env: 'prod',
        shopperName: 'Ada',
        shopperEmail: 'ada@example.com',
      })
    ).toEqual({
      intent_id: 'int_1',
      client_secret: 'secret_1',
      currency: 'USD',
      env: 'prod',
      disableAutoRedirect: true,
      shopper_name: 'Ada',
      shopper_email: 'ada@example.com',
    });
  });

  test('omits empty shopper fields', () => {
    expect(
      buildHostedCheckoutOptions({ intentId: 'int_2', clientSecret: 'secret_2', currency: 'USD', env: 'demo' })
    ).toEqual({
      intent_id: 'int_2',
      client_secret: 'secret_2',
      currency: 'USD',
      env: 'demo',
      disableAutoRedirect: true,
    });
  });

  test('defaults to prod and only switches to demo explicitly', () => {
    expect(resolveAirwallexEnv(undefined)).toBe('prod');
    expect(resolveAirwallexEnv('PROD')).toBe('prod');
    expect(resolveAirwallexEnv('demo')).toBe('demo');
  });

  test('only opens Airwallex checkout origins in the system browser', () => {
    expect(isAirwallexCheckoutUrl('https://checkout.airwallex.com/#/standalone/checkout?intent_id=int_1')).toBe(true);
    expect(isAirwallexCheckoutUrl('https://checkout-demo.airwallex.com/#/standalone/checkout')).toBe(true);
    expect(isAirwallexCheckoutUrl('http://checkout.airwallex.com/')).toBe(false);
    expect(isAirwallexCheckoutUrl('https://checkout.airwallex.com.evil.test/')).toBe(false);
    expect(isAirwallexCheckoutUrl('javascript:alert(1)')).toBe(false);
    expect(isAirwallexCheckoutUrl(undefined)).toBe(false);
  });
});
