import { describe, expect, test } from 'bun:test';

import { needsWebKitSoftwareRaster } from './platform';

const originalWindow = globalThis.window;

const withWindow = (value: unknown, run: () => void): void => {
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    value,
  });

  try {
    run();
  } finally {
    if (typeof originalWindow === 'undefined') {
      Reflect.deleteProperty(globalThis, 'window');
    } else {
      Object.defineProperty(globalThis, 'window', {
        configurable: true,
        value: originalWindow,
      });
    }
  }
};

describe('needsWebKitSoftwareRaster', () => {
  test('is enabled only for the Linux desktop shell', () => {
    withWindow({ __backendPort: 25808, __os: 'linux' }, () => {
      expect(needsWebKitSoftwareRaster()).toBe(true);
    });
  });

  test('stays off for the Windows and macOS desktop shells', () => {
    withWindow({ __backendPort: 25808, __os: 'windows' }, () => {
      expect(needsWebKitSoftwareRaster()).toBe(false);
    });
    withWindow({ __backendPort: 25808, __os: 'macos' }, () => {
      expect(needsWebKitSoftwareRaster()).toBe(false);
    });
  });

  test('stays off for the WebUI browser even on Linux', () => {
    withWindow({}, () => {
      expect(needsWebKitSoftwareRaster()).toBe(false);
    });
  });
});
