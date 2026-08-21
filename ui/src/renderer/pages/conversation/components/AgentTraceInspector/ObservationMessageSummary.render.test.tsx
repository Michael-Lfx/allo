/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { describe, expect, test } from 'bun:test';
import { createInstance } from 'i18next';
import React from 'react';
import { I18nextProvider, initReactI18next } from 'react-i18next';
import { renderToStaticMarkup } from 'react-dom/server';
import conversation from '@/renderer/services/i18n/locales/zh-CN/conversation.json';
import { MessageScanList, messageScanPresentation } from './ObservationMessageSummary';
import { projectObservationScan } from './observationScan';

const testI18n = createInstance();
await testI18n.use(initReactI18next).init({
  lng: 'zh-CN',
  fallbackLng: 'zh-CN',
  resources: { 'zh-CN': { translation: { conversation } } },
  interpolation: { escapeValue: false },
});

const contextMessage = {
  role: 'user',
  content: [
    { type: 'text', text: '[Context]\nCurrent date: 2026-08-21' },
    { type: 'text', text: '66' },
  ],
};

describe('ObservationMessageSummary render contract', () => {
  test('renders only the real user text while keeping Context in the hover projection', () => {
    const result = projectObservationScan([contextMessage], 'messages');
    expect(result.kind).toBe('messages');
    if (result.kind !== 'messages') return;

    const html = renderToStaticMarkup(
      <I18nextProvider i18n={testI18n}>
        <MessageScanList rows={result.rows} newestFirst={false} />
      </I18nextProvider>,
    );
    const visiblePreviews = [
      ...html.matchAll(/<span class="session-logs-scan__preview">([^<]*)<\/span>/g),
    ].map((match) => match[1]);

    expect(html).toContain('>用户<');
    expect(visiblePreviews).toEqual(['66']);
    expect(visiblePreviews.join('')).not.toContain('[Context]');
    expect(visiblePreviews.join('')).not.toContain('Current date');

    const t = testI18n.t.bind(testI18n);
    const presentation = messageScanPresentation(result.rows[0]!, t);
    expect(presentation).toEqual({
      visibleText: '66',
      tooltipText: '上下文 · Current date: 2026-08-21',
      tipFallback: true,
    });
  });

  test('keeps a Context-only row without adding a missing-preview label', () => {
    const result = projectObservationScan(
      [
        {
          role: 'user',
          content: [{ type: 'text', text: '[Context]\nCurrent date: 2026-08-21' }],
        },
      ],
      'messages',
    );
    expect(result.kind).toBe('messages');
    if (result.kind !== 'messages') return;

    const html = renderToStaticMarkup(
      <I18nextProvider i18n={testI18n}>
        <MessageScanList rows={result.rows} newestFirst={false} />
      </I18nextProvider>,
    );
    const visiblePreviews = [
      ...html.matchAll(/<span class="session-logs-scan__preview">([^<]*)<\/span>/g),
    ].map((match) => match[1]);

    expect(html).toContain('>用户<');
    expect(visiblePreviews).toEqual(['']);
    expect(visiblePreviews.join('')).not.toContain('观测未记录');

    const t = testI18n.t.bind(testI18n);
    expect(messageScanPresentation(result.rows[0]!, t)).toEqual({
      visibleText: '',
      tooltipText: '上下文 · Current date: 2026-08-21',
      tipFallback: false,
    });
  });
});
