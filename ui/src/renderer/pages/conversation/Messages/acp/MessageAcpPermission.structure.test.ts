import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const source = readFileSync(new URL('./MessageAcpPermission.tsx', import.meta.url), 'utf8');

describe('MessageAcpPermission Approval Card adapter', () => {
  test('renders the Beautiful UI approval shell without changing confirmation IPC', () => {
    expect(source.includes('<ApprovalCard')).toBe(true);
    expect(source.includes("data-testid='message-acp-permission-card'")).toBe(true);
    expect(source.includes('conversation.confirmMessage.invoke')).toBe(true);
  });

  test('maps permission option kinds through i18n and keeps an explicit fallback', () => {
    expect(source.includes('ACP_PERMISSION_OPTION_I18N_KEYS')).toBe(true);
    expect(source.includes('messages.confirmation.rejectOnce')).toBe(true);
    expect(source.includes('messages.confirmation.rejectAlways')).toBe(true);
    expect(source.includes('hasOwnProperty.call(ACP_PERMISSION_OPTION_I18N_KEYS')).toBe(true);
    expect(source.includes('const label = translationKey ? t(translationKey')).toBe(true);
  });

  test('keeps ACP option identity, confirmation payload, and dynamic command content', () => {
    expect(source.includes('id: option_id')).toBe(true);
    expect(source.includes('confirm_key: selected')).toBe(true);
    expect(source.includes('call_id: toolCallId')).toBe(true);
    expect(source.includes('raw_input?.command || tool_call.title')).toBe(true);
  });
});
