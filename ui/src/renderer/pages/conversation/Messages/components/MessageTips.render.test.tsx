import { describe, expect, test } from 'bun:test';
import { createInstance } from 'i18next';
import React from 'react';
import { I18nextProvider, initReactI18next } from 'react-i18next';
import { renderToStaticMarkup } from 'react-dom/server';
import { MemoryRouter } from 'react-router-dom';

import type { IMessageTips, IMessageText, TMessage } from '@/common/chat/chatLib';
import { parseConversationId, parseMessageId } from '@/common/types/ids';
import { ConversationProvider } from '@/renderer/hooks/context/ConversationContext';
import common from '@/renderer/services/i18n/locales/zh-CN/common.json';
import conversation from '@/renderer/services/i18n/locales/zh-CN/conversation.json';
import settings from '@/renderer/services/i18n/locales/zh-CN/settings.json';
import { MessageListProvider } from '../hooks';
import MessageTips from './MessageTips';

const testI18n = createInstance();
await testI18n.use(initReactI18next).init({
  lng: 'zh-CN',
  fallbackLng: 'zh-CN',
  resources: {
    'zh-CN': { translation: { common, conversation, settings } },
  },
  interpolation: { escapeValue: false },
});

const conversationId = parseConversationId('019b0000-0000-7000-8000-000000000911');
const userMessageId = parseMessageId('019b0000-0000-7000-8000-000000000912');
const errorMessageId = parseMessageId('019b0000-0000-7000-8000-000000000913');

const renderMessage = (error?: IMessageTips['content']['error']): string => {
  const userMessage: IMessageText = {
    id: 'message-tips-user',
    msg_id: userMessageId,
    conversation_id: conversationId,
    type: 'text',
    content: { content: '你好' },
    position: 'right',
    created_at: 100,
  };
  const errorMessage: IMessageTips = {
    id: 'message-tips-error',
    msg_id: errorMessageId,
    conversation_id: conversationId,
    type: 'tips',
    content: {
      content: error?.detail ?? error?.message ?? '请求失败，请稍后重试。',
      type: 'error',
      ...(error ? { error } : {}),
    },
    position: 'left',
    created_at: 200,
  };
  const messages: TMessage[] = [userMessage, errorMessage];

  return renderToStaticMarkup(
    <I18nextProvider i18n={testI18n}>
      <MemoryRouter>
        <ConversationProvider value={{ conversation_id: conversationId, type: 'nomi', readOnly: false }}>
          <MessageListProvider value={messages}>
            <MessageTips message={errorMessage} />
          </MessageListProvider>
        </ConversationProvider>
      </MemoryRouter>
    </I18nextProvider>
  );
};

describe('MessageTips error render contract', () => {
  test('renders the real error card with summary, collapsed details, and ordered actions', () => {
    const html = renderMessage({
      message: '模型服务商拒绝了工具定义',
      model_id: 'claude-sonnet-4-20250514',
      code: 'USER_LLM_PROVIDER_INVALID_TOOL_SCHEMA',
      incident_id: 'incident-message-tips-1',
      ownership: 'user_llm_provider',
      retryable: true,
      resolution: { kind: 'retry' },
      detail: 'Invalid schema for function Read',
    });

    const retryIndex = html.indexOf('message-error-note__retry');
    const editIndex = html.indexOf('data-testid="message-error-edit"');
    const copyIndex = html.indexOf('message-error-note__copy');
    const feedbackIndex = html.indexOf('message-error-note__feedback');

    expect(html).toContain('message-error-note__diagnostic-summary');
    expect(html).toContain('模型 ID');
    expect(html).toContain('claude-sonnet-4-20250514');
    expect(html).toContain('Invalid schema for function Read');
    expect(html).not.toContain('message-error-note__tag');
    expect(html).not.toContain('message-error-note__resolution');
    expect(html).not.toContain('message-error-note__incident');
    expect(html).toContain('message-error-note__details');
    expect(html).not.toContain('arco-collapse-item-active');
    expect(retryIndex).toBeGreaterThan(-1);
    expect(editIndex).toBeGreaterThan(retryIndex);
    expect(copyIndex).toBeGreaterThan(editIndex);
    expect(feedbackIndex).toBeGreaterThan(copyIndex);
  });

  test('keeps legacy unstructured errors renderable with a safe fallback summary', () => {
    const html = renderMessage();

    expect(html).toContain('message-error-note');
    expect(html).toContain('请求失败，请稍后重试。');
    expect(html).toContain('message-error-note__copy');
  });

  test('uses the configuration recovery action before edit, copy, and feedback', () => {
    const html = renderMessage({
      message: '模型配置不可用',
      code: 'PROVIDER_UNAVAILABLE',
      ownership: 'user_llm_provider',
      retryable: false,
      resolution: { kind: 'change_model', target: 'provider_settings' },
    });

    const recoveryIndex = html.indexOf('data-testid="message-error-recovery"');
    const copyIndex = html.indexOf('message-error-note__copy');
    const feedbackIndex = html.indexOf('message-error-note__feedback');

    expect(recoveryIndex).toBeGreaterThan(-1);
    expect(copyIndex).toBeGreaterThan(recoveryIndex);
    expect(feedbackIndex).toBeGreaterThan(copyIndex);
  });

  test('keeps standalone recovery guidance when no direct recovery action is available', () => {
    const html = renderMessage({
      message: '会话正在处理上一条消息',
      code: 'NOMIFUN_CONVERSATION_BUSY',
      retryable: false,
      resolution: { kind: 'wait_for_current_response' },
    });

    expect(html).toContain('message-error-note__resolution');
    expect(html).toContain('建议：');
    expect(html).not.toContain('message-error-note__retry');
  });

  test('uses the billing recovery action instead of changing the model', () => {
    const html = renderMessage({
      message: '积分不足',
      code: 'USER_LLM_PROVIDER_BILLING_REQUIRED',
      ownership: 'user_llm_provider',
      retryable: false,
      resolution: { kind: 'check_provider_billing', target: 'provider_settings' },
    });

    expect(html).toContain('data-testid="message-error-recovery"');
    expect(html).toContain('购买积分');
    expect(html).not.toContain('更换模型');
    expect(html).not.toContain('/models?section=models');
    expect(html).toContain('message-error-note__copy');
    expect(html).toContain('message-error-note__feedback');
    expect(html).not.toContain('message-error-note__retry');
  });
});
