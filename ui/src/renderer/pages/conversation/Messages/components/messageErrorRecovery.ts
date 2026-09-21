import type { AgentStreamErrorInfo } from '@/common/chat/chatLib';

export type MessageErrorRecoveryAction =
  | {
      labelKey: string;
      source: 'open_billing';
    }
  | {
      labelKey: string;
      href: string;
      source: 'change_model';
    }
  | {
      labelKey: string;
      href: string;
      source: 'fix_agent_config';
    };

const MODEL_RECOVERY_KINDS = new Set([
  'change_model',
  'check_provider_credentials',
  'check_provider_base_url',
]);

const AGENT_RECOVERY_KINDS = new Set([
  'reconnect_agent',
  'check_agent_login',
  'check_agent_installation',
  'check_agent_version',
  'check_local_command',
]);

export function resolveMessageErrorRecoveryAction(
  error: AgentStreamErrorInfo | undefined
): MessageErrorRecoveryAction | null {
  const kind = error?.resolution?.kind;
  if (!kind) return null;

  if (kind === 'check_provider_billing') {
    return {
      labelKey: 'conversation.agentError.openBillingAction',
      source: 'open_billing',
    };
  }

  if (MODEL_RECOVERY_KINDS.has(kind) || error.code === 'PROVIDER_UNAVAILABLE') {
    return {
      labelKey: 'conversation.agentError.changeModelAction',
      href: '/models?section=models',
      source: 'change_model',
    };
  }

  if (AGENT_RECOVERY_KINDS.has(kind)) {
    return {
      labelKey: 'conversation.agentError.fixConfigAction',
      href: '/models?section=agents',
      source: 'fix_agent_config',
    };
  }

  return null;
}
