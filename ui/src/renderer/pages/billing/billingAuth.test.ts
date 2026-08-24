import { describe, expect, test } from 'bun:test';
import {
  BILLING_PATH,
  buildBillingCheckoutSuccessUrl,
  clearPendingBillingOrder,
  cloudLoginRedirectForPath,
  isSafePostCloudLoginPath,
  persistPendingBillingOrder,
  readPendingBillingCheckout,
  readPendingBillingOrder,
  resolvePostCloudLoginPath,
} from './billingAuth';

const memoryStore = (initial: Record<string, string> = {}) => {
  const data = { ...initial };
  return {
    getItem: (key: string) => (key in data ? data[key] : null),
    setItem: (key: string, value: string) => {
      data[key] = value;
    },
    removeItem: (key: string) => {
      delete data[key];
    },
  };
};

describe('billing auth return path', () => {
  test('only /billing is a safe post-login destination', () => {
    expect(isSafePostCloudLoginPath(BILLING_PATH)).toBe(true);
    expect(isSafePostCloudLoginPath('/guid')).toBe(false);
    expect(isSafePostCloudLoginPath('https://evil.example')).toBe(false);
    expect(isSafePostCloudLoginPath('//evil.example')).toBe(false);
    expect(isSafePostCloudLoginPath(null)).toBe(false);
  });

  test('preserves /billing across the cloud-login bounce', () => {
    expect(cloudLoginRedirectForPath(BILLING_PATH)).toBe('/cloud-login?next=%2Fbilling');
    expect(cloudLoginRedirectForPath('/guid')).toBe('/cloud-login');
    expect(resolvePostCloudLoginPath('?next=/billing')).toBe(BILLING_PATH);
    expect(resolvePostCloudLoginPath('next=/billing')).toBe(BILLING_PATH);
    expect(resolvePostCloudLoginPath('?next=/guid')).toBe('/guid');
    expect(resolvePostCloudLoginPath('')).toBe('/guid');
  });
});

describe('Airwallex hosted checkout return', () => {
  test('builds a HashRouter success URL back to /billing with the order number', () => {
    expect(buildBillingCheckoutSuccessUrl('http://localhost:5173', 'ORD-1')).toBe(
      'http://localhost:5173/#/billing?orderNo=ORD-1'
    );
    expect(buildBillingCheckoutSuccessUrl('https://tauri.localhost/', 'a b')).toBe(
      'https://tauri.localhost/#/billing?orderNo=a%20b'
    );
  });

  test('uses a stored checkout only to fill in the matching URL orderNo', () => {
    const store = memoryStore({ 'flowy.billing.pendingOrderNo': 'stored-order' });
    expect(readPendingBillingOrder('?orderNo=from-query', store)).toBe('from-query');
    expect(readPendingBillingOrder('', store)).toBe('');
    persistPendingBillingOrder('next-order', store);
    expect(readPendingBillingOrder('?orderNo=next-order', store)).toBe('next-order');
    persistPendingBillingOrder(
      { orderNo: 'ORD-9', intentId: 'int_9', clientSecret: 'secret_9', currency: 'USD' },
      store
    );
    expect(readPendingBillingOrder('', store)).toBe('');
    expect(readPendingBillingOrder('?orderNo=ORD-9', store)).toBe('ORD-9');
    clearPendingBillingOrder(store);
    expect(readPendingBillingOrder('', store)).toBe('');
  });

  test('does not resume a stored checkout unless the URL has the same orderNo', () => {
    const store = memoryStore();
    persistPendingBillingOrder(
      { orderNo: 'ORD-9', intentId: 'int_9', clientSecret: 'secret_9', currency: 'USD' },
      store
    );
    expect(readPendingBillingOrder('', store)).toBe('');
    expect(readPendingBillingOrder('?orderNo=ORD-other', store)).toBe('ORD-other');
    expect(readPendingBillingOrder('?orderNo=ORD-9', store)).toBe('ORD-9');
    expect(readPendingBillingCheckout('?orderNo=ORD-9', store)).toEqual({
      orderNo: 'ORD-9',
      intentId: 'int_9',
      clientSecret: 'secret_9',
      currency: 'USD',
    });
    expect(readPendingBillingCheckout('?orderNo=ORD-other', store)).toEqual({ orderNo: 'ORD-other' });
    clearPendingBillingOrder(store);
    expect(readPendingBillingOrder('?orderNo=ORD-9', store)).toBe('ORD-9');
  });
});
