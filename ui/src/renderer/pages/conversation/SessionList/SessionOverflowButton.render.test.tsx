import { describe, expect, test } from 'bun:test';
import { createInstance } from 'i18next';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { I18nextProvider, initReactI18next } from 'react-i18next';

import sessionListZh from '@/renderer/services/i18n/locales/zh-CN/sessionList.json';

import SessionOverflowButton from './SessionOverflowButton';

const testI18n = createInstance();
await testI18n.use(initReactI18next).init({
  lng: 'zh-CN',
  fallbackLng: 'zh-CN',
  resources: { 'zh-CN': { translation: { sessionList: sessionListZh } } },
  interpolation: { escapeValue: false },
});

const renderButton = (props: React.ComponentProps<typeof SessionOverflowButton>): string =>
  renderToStaticMarkup(
    <I18nextProvider i18n={testI18n}>
      <SessionOverflowButton {...props} />
    </I18nextProvider>
  );

describe('SessionOverflowButton render behavior', () => {
  test('renders count-aware collapsed copy and disclosure attributes', () => {
    const markup = renderButton({
      expanded: false,
      hiddenCount: 3,
      controlsId: 'flowy-overflow-test',
      onToggle: () => undefined,
    });

    expect(markup).toContain('aria-expanded="false"');
    expect(markup).toContain('aria-controls="flowy-overflow-test"');
    expect(markup).toContain('展开 3 条');
  });

  test('renders collapse copy when expanded', () => {
    const markup = renderButton({
      expanded: true,
      hiddenCount: 3,
      controlsId: 'flowy-overflow-test',
      onToggle: () => undefined,
    });

    expect(markup).toContain('aria-expanded="true"');
    expect(markup).toContain('收起');
    expect(markup).not.toContain('展开 3 条');
  });

  test('renders nothing when there are no hidden entries', () => {
    expect(
      renderButton({
        expanded: false,
        hiddenCount: 0,
        controlsId: 'flowy-overflow-test',
        onToggle: () => undefined,
      })
    ).toBe('');
  });
});
