import { describe, expect, test } from 'bun:test';
import { createInstance } from 'i18next';
import React from 'react';
import { I18nextProvider, initReactI18next } from 'react-i18next';
import { renderToStaticMarkup } from 'react-dom/server';
import { MemoryRouter } from 'react-router-dom';

import billing from '@/renderer/services/i18n/locales/zh-CN/billing.json';
import videoGeneration from '@/renderer/services/i18n/locales/zh-CN/videoGeneration.json';
import type { FailureKind } from '../classifyFailure';
import StudioSessionMessageView from './StudioSessionMessage';
import type { StudioSessionMessage } from './types';

const testI18n = createInstance();
await testI18n.use(initReactI18next).init({
  lng: 'zh-CN',
  fallbackLng: 'zh-CN',
  resources: {
    'zh-CN': { translation: { billing, videoGeneration } },
  },
  interpolation: { escapeValue: false },
});

const failureItem: StudioSessionMessage = {
  id: 'fail-credits',
  role: 'error',
  kind: 'failure',
  error: 'INSUFFICIENT_CREDITS',
};

const renderFailure = (issueKind?: FailureKind) =>
  renderToStaticMarkup(
    <I18nextProvider i18n={testI18n}>
      <MemoryRouter>
        <StudioSessionMessageView
          sessionId='session-1'
          item={failureItem}
          title='积分不足'
          body='当前积分不足以完成本次生成。'
          issueKind={issueKind}
        />
      </MemoryRouter>
    </I18nextProvider>
  );

describe('studio session failure card render', () => {
  test('credits failures render a buy-credits action', () => {
    const html = renderFailure('credits');
    expect(html).toContain('data-testid="video-failure-open-billing"');
    expect(html).toContain('购买积分');
  });

  test('other failures do not render a billing CTA', () => {
    const html = renderFailure('llm');
    expect(html).not.toContain('data-testid="video-failure-open-billing"');
    expect(html).not.toContain('购买积分');
  });

  test('cancelled cards do not render a billing CTA', () => {
    const html = renderToStaticMarkup(
      <I18nextProvider i18n={testI18n}>
        <MemoryRouter>
          <StudioSessionMessageView
            sessionId='session-1'
            item={{ id: 'cancelled-1', role: 'error', kind: 'cancelled' }}
            title='已停止'
            issueKind='credits'
          />
        </MemoryRouter>
      </I18nextProvider>
    );
    expect(html).not.toContain('data-testid="video-failure-open-billing"');
  });
});
