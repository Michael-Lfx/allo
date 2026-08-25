import { describe, expect, test } from 'bun:test';
import type { AgentStreamErrorInfo } from '@/common/chat/chatLib';
import { resolveMessageErrorRecoveryAction } from './messageErrorRecovery';

const error = (resolution: AgentStreamErrorInfo['resolution'], code?: string): AgentStreamErrorInfo => ({
  message: 'Request failed',
  ...(code ? { code } : {}),
  resolution,
});

describe('message error recovery action', () => {
  test('always opens billing for provider credit errors', () => {
    expect(
      resolveMessageErrorRecoveryAction(error({ kind: 'check_provider_billing', target: 'provider_settings' }))
    ).toEqual({
      labelKey: 'conversation.agentError.openBillingAction',
      href: '/billing',
      source: 'open_billing',
    });
  });

  test('preserves a genuine model change action', () => {
    expect(resolveMessageErrorRecoveryAction(error({ kind: 'change_model' }))).toEqual({
      labelKey: 'conversation.agentError.changeModelAction',
      href: '/models?section=models',
      source: 'change_model',
    });
  });

  test('preserves the agent configuration action', () => {
    expect(resolveMessageErrorRecoveryAction(error({ kind: 'check_agent_login' }))).toEqual({
      labelKey: 'conversation.agentError.fixConfigAction',
      href: '/models?section=agents',
      source: 'fix_agent_config',
    });
  });

  test('does not create an action for unrelated errors', () => {
    expect(resolveMessageErrorRecoveryAction(error({ kind: 'send_feedback' }))).toBeNull();
  });
});
