import { describe, expect, test } from 'bun:test';
import { createInstance } from 'i18next';
import React from 'react';
import { I18nextProvider, initReactI18next } from 'react-i18next';
import { renderToStaticMarkup } from 'react-dom/server';

import common from '@/renderer/services/i18n/locales/zh-CN/common.json';
import conversation from '@/renderer/services/i18n/locales/zh-CN/conversation.json';
import settings from '@/renderer/services/i18n/locales/zh-CN/settings.json';
import ErrorDiagnosticContent from './ErrorDiagnosticContent';
import { buildAgentErrorDiagnostic, buildUnknownErrorDiagnostic } from '@/renderer/utils/ui/errorDiagnostics';

const testI18n = createInstance();
await testI18n.use(initReactI18next).init({
  lng: 'zh-CN',
  fallbackLng: 'zh-CN',
  resources: {
    'zh-CN': { translation: { common, conversation, settings } },
  },
  interpolation: { escapeValue: false },
});

const renderDiagnostic = (detail?: string): string =>
  renderToStaticMarkup(
    <I18nextProvider i18n={testI18n}>
      <ErrorDiagnosticContent
        diagnostic={buildAgentErrorDiagnostic({
          message: '模型服务商拒绝了请求',
          code: 'USER_LLM_PROVIDER_INVALID_TOOL_SCHEMA',
          incident_id: 'incident-render-1',
          ownership: 'user_llm_provider',
          retryable: true,
          detail,
        })}
      />
    </I18nextProvider>
  );

describe('ErrorDiagnosticContent render contract', () => {
  test('keeps the safe summary visible and details collapsed by default', () => {
    const html = renderDiagnostic('Invalid schema for function Read');

    expect(html).toContain('conversation-error-diagnostic__summary');
    expect(html).toContain('Invalid schema for function Read');
    expect(html).toContain('conversation-error-diagnostic__detail');
    expect(html).toContain('复制诊断');
    expect(html).not.toContain('arco-collapse-item-active');
  });

  test('renders localized metadata and keeps sensitive paths out of the copied detail', () => {
    const html = renderDiagnostic('workspace_path=C:\\Users\\secret\\workspace\\project\nToken=secret');

    expect(html).toContain('错误码');
    expect(html).toContain('USER_LLM_PROVIDER_INVALID_TOOL_SCHEMA');
    const meta = html.match(/<div class="conversation-error-diagnostic__meta">([\s\S]*?)<\/div>/)?.[1] ?? '';
    expect(meta).not.toContain('incident-render-1');
    expect(html).not.toContain('关联 ID');
    expect(html).not.toContain('C:\\Users\\secret\\workspace\\project');
    expect(html).not.toContain('Token=secret');
  });

  test('surfaces BackendHttpError status and code without rendering the raw error object', () => {
    const error = Object.assign(new Error('transport message'), {
      name: 'BackendHttpError',
      status: 503,
      code: 'PROVIDER_UNAVAILABLE',
      backendMessage: '服务暂时不可用',
      details: { reason: 'upstream unavailable', token: 'Bearer secret' },
    });
    const html = renderToStaticMarkup(
      <I18nextProvider i18n={testI18n}>
        <ErrorDiagnosticContent diagnostic={buildUnknownErrorDiagnostic(error, '请求失败')} />
      </I18nextProvider>
    );

    expect(html).toContain('HTTP 状态');
    expect(html).toContain('503');
    expect(html).toContain('PROVIDER_UNAVAILABLE');
    expect(html).toContain('服务暂时不可用');
    expect(html).not.toContain('Bearer secret');
  });
});
