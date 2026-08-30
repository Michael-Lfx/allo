/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';

import { rewriteCatalogIconUrl } from './catalogIcon';

describe('rewriteCatalogIconUrl', () => {
  test('keeps absolute http(s) and data URLs', () => {
    expect(rewriteCatalogIconUrl('https://cdn.example/seedream.png')).toBe(
      'https://cdn.example/seedream.png'
    );
    expect(rewriteCatalogIconUrl('http://cdn.example/a.png', 'https://api.flowy')).toBe(
      'http://cdn.example/a.png'
    );
    expect(rewriteCatalogIconUrl('data:image/png;base64,abc')).toBe('data:image/png;base64,abc');
  });

  test('joins server-relative catalog icons to the Flowy origin', () => {
    expect(rewriteCatalogIconUrl('/static/seedream.png', 'https://api.flowy.example/')).toBe(
      'https://api.flowy.example/static/seedream.png'
    );
    expect(rewriteCatalogIconUrl('models/seedance.png', 'https://api.flowy.example')).toBe(
      'https://api.flowy.example/models/seedance.png'
    );
  });

  test('promotes protocol-relative URLs to https', () => {
    expect(rewriteCatalogIconUrl('//cdn.example/icon.png')).toBe('https://cdn.example/icon.png');
  });

  test('returns empty for blank icons', () => {
    expect(rewriteCatalogIconUrl('')).toBe('');
    expect(rewriteCatalogIconUrl('   ')).toBe('');
    expect(rewriteCatalogIconUrl(undefined)).toBe('');
  });
});
