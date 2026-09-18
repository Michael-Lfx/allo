import { afterEach, describe, expect, test } from 'vitest';

import { installLegacyWebKitPolyfills } from './legacyWebKit';

const nativeMethods = {
  findLast: Array.prototype.findLast,
  findLastIndex: Array.prototype.findLastIndex,
  toReversed: Array.prototype.toReversed,
  toSorted: Array.prototype.toSorted,
};

afterEach(() => {
  for (const [name, implementation] of Object.entries(nativeMethods)) {
    Object.defineProperty(Array.prototype, name, {
      configurable: true,
      writable: true,
      value: implementation,
    });
  }
});

describe('legacy WebKit compatibility', () => {
  test('installs non-mutating array methods missing from Safari 15.5', () => {
    for (const name of Object.keys(nativeMethods)) {
      Reflect.deleteProperty(Array.prototype, name);
    }

    installLegacyWebKitPolyfills();

    const values = [3, 1, 2];
    expect(values.toReversed()).toEqual([2, 1, 3]);
    expect(values.toSorted()).toEqual([1, 2, 3]);
    expect(values).toEqual([3, 1, 2]);
    expect(values.findLast((value) => value < 3)).toBe(2);
    expect(values.findLastIndex((value) => value < 3)).toBe(2);
  });

  test('does not replace methods supplied by newer WebKit versions', () => {
    const current = Array.prototype.toReversed;
    installLegacyWebKitPolyfills();
    expect(Array.prototype.toReversed).toBe(current);
  });
});
