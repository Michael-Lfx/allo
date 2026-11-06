/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { afterEach, describe, expect, test } from 'bun:test';
import { browseDirectory, createDirectory } from './directorySelectionApi';

const realFetch = Object.getOwnPropertyDescriptor(globalThis, 'fetch');
const realWindow = Object.getOwnPropertyDescriptor(globalThis, 'window');
const realDocument = Object.getOwnPropertyDescriptor(globalThis, 'document');

function restoreGlobal(name: 'fetch' | 'window' | 'document', descriptor?: PropertyDescriptor): void {
  if (descriptor) Object.defineProperty(globalThis, name, descriptor);
  else Reflect.deleteProperty(globalThis, name);
}

function installWebUiGlobals(csrfToken = ''): void {
  Object.defineProperty(globalThis, 'window', {
    configurable: true,
    writable: true,
    value: {
      location: {
        origin: 'http://192.168.3.68:25808',
        protocol: 'http:',
        host: '192.168.3.68:25808',
        pathname: '/#/guid',
        hash: '#/guid',
      },
      dispatchEvent: () => true,
      addEventListener: () => undefined,
      removeEventListener: () => undefined,
    },
  });
  Object.defineProperty(globalThis, 'document', {
    configurable: true,
    writable: true,
    value: { cookie: csrfToken ? `nomifun-csrf-token=${csrfToken}` : '' },
  });
}

afterEach(() => {
  if (realFetch) Object.defineProperty(globalThis, 'fetch', realFetch);
  restoreGlobal('window', realWindow);
  restoreGlobal('document', realDocument);
});

describe('directorySelectionApi WebUI request security', () => {
  test('createDirectory carries the double-submit CSRF token through the shared bridge', async () => {
    installWebUiGlobals('csrf-value');
    let capturedUrl = '';
    let capturedInit: RequestInit | undefined;
    globalThis.fetch = ((url: string | URL | Request, init?: RequestInit) => {
      capturedUrl = String(url);
      capturedInit = init;
      return Promise.resolve(
        new Response(
          JSON.stringify({
            success: true,
            data: { name: 'nomi', path: 'C:\\study\\nomi', isDirectory: true, isFile: false },
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } }
        )
      );
    }) as typeof fetch;

    const created = await createDirectory('C:\\study', 'nomi');
    const headers = new Headers(capturedInit?.headers);

    expect(capturedUrl).toBe('/api/fs/directory');
    expect(capturedInit?.method).toBe('POST');
    expect(headers.get('content-type')).toBe('application/json');
    expect(headers.get('x-csrf-token')).toBe('csrf-value');
    expect(JSON.parse(String(capturedInit?.body))).toEqual({ parentPath: 'C:\\study', name: 'nomi' });
    expect(created.path).toBe('C:\\study\\nomi');
  });

  test('createDirectory self-heals once when the backend refreshes a missing CSRF cookie', async () => {
    installWebUiGlobals();
    const csrfHeaders: Array<string | null> = [];
    globalThis.fetch = ((_url: string | URL | Request, init?: RequestInit) => {
      const headers = new Headers(init?.headers);
      csrfHeaders.push(headers.get('x-csrf-token'));
      if (csrfHeaders.length === 1) {
        (globalThis as { document?: { cookie: string } }).document!.cookie =
          'nomifun-csrf-token=fresh-token';
        return Promise.resolve(
          new Response(
            JSON.stringify({ success: false, error: 'Forbidden: CSRF token validation failed', code: 'FORBIDDEN' }),
            { status: 403, headers: { 'Content-Type': 'application/json' } }
          )
        );
      }
      return Promise.resolve(
        new Response(
          JSON.stringify({
            success: true,
            data: { name: 'nomi', path: 'C:\\study\\nomi', isDirectory: true, isFile: false },
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } }
        )
      );
    }) as typeof fetch;

    await createDirectory('C:\\study', 'nomi');

    expect(csrfHeaders).toEqual([null, 'fresh-token']);
  });

  test('browseDirectory remains a cache-bypassing GET with encoded picker parameters', async () => {
    installWebUiGlobals('csrf-value');
    let capturedUrl = '';
    let capturedInit: RequestInit | undefined;
    globalThis.fetch = ((url: string | URL | Request, init?: RequestInit) => {
      capturedUrl = String(url);
      capturedInit = init;
      return Promise.resolve(
        new Response(
          JSON.stringify({ success: true, data: { items: [], currentPath: 'C:\\study', canGoUp: true } }),
          { status: 200, headers: { 'Content-Type': 'application/json' } }
        )
      );
    }) as typeof fetch;

    await browseDirectory('C:\\study folder', false);

    expect(capturedUrl).toBe('/api/fs/browse?path=C%3A%5Cstudy%20folder&showFiles=false');
    expect(capturedInit?.method).toBe('GET');
    expect(capturedInit?.cache).toBe('no-store');
    expect(new Headers(capturedInit?.headers).has('x-csrf-token')).toBe(false);
  });
});
