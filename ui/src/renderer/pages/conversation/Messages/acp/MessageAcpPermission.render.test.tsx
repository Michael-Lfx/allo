import { createInstance } from 'i18next';
import { I18nextProvider, initReactI18next } from 'react-i18next';
import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, test } from 'bun:test';
import enMessages from '@/renderer/services/i18n/locales/en-US/messages.json';
import zhMessages from '@/renderer/services/i18n/locales/zh-CN/messages.json';
import { transformMessage, type IMessageAcpPermission } from '@/common/chat/chatLib';
import type { AcpPermissionOptionKind } from '@/common/types/platform/acpTypes';
import { parseConversationId, parseMessageId } from '@/common/types/ids';
import MessageAcpPermission from './MessageAcpPermission';

const createPermissionMessage = (): IMessageAcpPermission => {
  const message = transformMessage({
    msg_id: parseMessageId('019b0000-0000-7000-8000-000000000001'),
    conversation_id: parseConversationId('0190f5fe-7c00-7a00-8000-000000000001'),
    type: 'acp_permission',
    data: {
      session_id: 'session-1',
      options: [
        { option_id: 'allow-once', name: 'Allow once', kind: 'allow_once' },
        { option_id: 'allow-always', name: 'Allow always', kind: 'allow_always' },
        { option_id: 'reject-once', name: 'Reject once', kind: 'reject_once' },
        { option_id: 'reject-always', name: 'Reject always', kind: 'reject_always' },
        { option_id: 'custom', name: 'Custom provider action', kind: 'provider_extension' },
      ],
      tool_call: {
        tool_call_id: 'tool-1',
        title: 'Write file',
        raw_input: { command: 'echo keep original' },
      },
    },
  });

  if (message?.type !== 'acp_permission') throw new Error('expected ACP permission message');
  return message;
};

const createPermissionMessageWithUnknownKinds = (...kinds: string[]): IMessageAcpPermission => {
  const message = createPermissionMessage();
  return {
    ...message,
    content: {
      ...message.content,
      options: [
        ...message.content.options,
        ...kinds.map((kind) => ({
          option_id: `unknown-${kind}`,
          name: `Original ${kind} action`,
          kind: kind as unknown as AcpPermissionOptionKind,
        })),
      ],
    },
  };
};

const renderPermission = async (
  language: 'zh-CN' | 'en-US',
  message: IMessageAcpPermission = createPermissionMessage()
) => {
  const i18n = createInstance();
  const messages = language === 'zh-CN' ? zhMessages : enMessages;
  await i18n.use(initReactI18next).init({
    lng: language,
    resources: { [language]: { translation: { messages } } },
    interpolation: { escapeValue: false },
  });

  return renderToStaticMarkup(
    <I18nextProvider i18n={i18n}>
      <MessageAcpPermission message={message} />
    </I18nextProvider>
  );
};

describe('MessageAcpPermission i18n', () => {
  test('renders standard ACP permission options in Chinese and keeps custom names', async () => {
    const html = await renderPermission('zh-CN');

    expect(html).toContain('是，允许一次');
    expect(html).toContain('是，始终允许');
    expect(html).toContain('否，仅此次拒绝');
    expect(html).toContain('否，始终拒绝');
    expect(html).toContain('Custom provider action');
    expect(html).not.toContain('Allow once');
    expect(html).not.toContain('Allow always');
    expect(html).not.toContain('Reject once');
    expect(html).not.toContain('Reject always');
    expect(html).toContain('echo keep original');
  });

  test('renders standard ACP permission options in English', async () => {
    const html = await renderPermission('en-US');

    expect(html).toContain('Yes, allow once');
    expect(html).toContain('Yes, allow always');
    expect(html).toContain('No, reject once');
    expect(html).toContain('No, always reject');
    expect(html).toContain('Custom provider action');
    expect(html).toContain('echo keep original');
  });

  test('keeps unrecognized ACP kind names as raw labels', async () => {
    const html = await renderPermission(
      'zh-CN',
      createPermissionMessageWithUnknownKinds('provider_extension', 'toString')
    );

    expect(html).toContain('Original provider_extension action');
    expect(html).toContain('Original toString action');
  });
});
