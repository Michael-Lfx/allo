/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('Nomi session metrics panel notice', () => {
  test('renders a data reliability notice in the metrics panel', () => {
    const source = readSource(new URL('./NomiSessionMetricsPanel.tsx', import.meta.url));

    expect(source.includes('conversation.sessionMetrics.notice')).toBe(true);
  });

  test('uses the required Chinese notice copy', () => {
    const zh = JSON.parse(readSource(new URL('../../../../services/i18n/locales/zh-CN/conversation.json', import.meta.url)));

    expect(zh.sessionMetrics.notice).toBe('因数据采集手段问题，数据仅供参考，不可作为定论');
  });

  test('rehydrates persisted usage when the metrics tab mounts', () => {
    const source = readSource(new URL('./NomiSessionMetricsPanel.tsx', import.meta.url));

    expect(source.includes('getConversationOrNull')).toBe(true);
    expect(source.includes('getPersistedUsage(latest)')).toBe(true);
  });
});

describe('conversation update body must not forward merge_extra', () => {
  test('ipcBridge conversation.update omits the client-only merge_extra flag', () => {
    const source = readSource(new URL('../../../../../common/adapter/ipcBridge.ts', import.meta.url));
    const updateStart = source.indexOf('update: httpPatch<boolean, { id: ConversationId;');
    expect(updateStart).toBeGreaterThan(-1);
    const updateChunk = source.slice(updateStart, updateStart + 700);

    expect(updateChunk.includes('deny_unknown_fields')).toBe(true);
    expect(updateChunk.includes('merge_extra: p.merge_extra')).toBe(false);
  });
});
