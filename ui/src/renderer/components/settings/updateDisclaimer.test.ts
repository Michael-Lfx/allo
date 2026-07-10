/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('update disclaimer', () => {
  test('keeps the requested nonprofit data-loss disclaimer fixed in Chinese locale file', () => {
    const zhUpdate = JSON.parse(readSource(new URL('../../services/i18n/locales/zh-CN/update.json', import.meta.url)));

    expect(zhUpdate.disclaimer).toBe(
      '免责声明：这是一个公益免费开源项目，故项目作者不承担任何版本迭代导致用户数据丢失、损坏的后果，请谨慎进行升级。'
    );
  });

  test('does not render the disclaimer in the update modal', () => {
    const updateModalSource = readSource(new URL('./UpdateModal.tsx', import.meta.url));

    expect(updateModalSource.includes('renderDisclaimer')).toBe(false);
    expect(updateModalSource.includes('update.disclaimer')).toBe(false);
  });
});
