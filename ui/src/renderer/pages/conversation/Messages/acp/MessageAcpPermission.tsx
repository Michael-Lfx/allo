
import { parseConfirmationCorrelationId, type IMessageAcpPermission } from '@/common/chat/chatLib';
import { optionalDisplayText, toDisplayText } from '@/common/chat/displayText';
import type { AcpPermissionOptionKind } from '@/common/types/platform/acpTypes';
import { conversation } from '@/common/adapter/ipcBridge';
import { useConversationContextSafe } from '@/renderer/hooks/context/ConversationContext';
import type { I18nKey } from '@/renderer/services/i18n';
import ApprovalCard from '@renderer/components/beautifulUi/approvalCard/ApprovalCard';
import { kindFromPermissionAction } from '@renderer/components/beautifulUi/approvalCard/approvalCardModel';
import { Typography } from '@arco-design/web-react';
import React, { useState } from 'react';
import { useTranslation } from 'react-i18next';

const { Text } = Typography;

const ACP_PERMISSION_OPTION_I18N_KEYS: Record<AcpPermissionOptionKind, I18nKey> = {
  allow_once: 'messages.confirmation.yesAllowOnce',
  allow_always: 'messages.confirmation.yesAllowAlways',
  reject_once: 'messages.confirmation.rejectOnce',
  reject_always: 'messages.confirmation.rejectAlways',
};

const isAcpPermissionOptionKind = (value: string): value is AcpPermissionOptionKind =>
  Object.prototype.hasOwnProperty.call(ACP_PERMISSION_OPTION_I18N_KEYS, value);

interface MessageAcpPermissionProps {
  message: IMessageAcpPermission;
}

const MessageAcpPermission: React.FC<MessageAcpPermissionProps> = React.memo(({ message }) => {
  const { options = [], tool_call } = message.content || {};
  const { t } = useTranslation();
  const readOnly = useConversationContextSafe()?.readOnly === true;

  const getToolInfo = () => {
    if (!tool_call) {
      return {
        title: t('messages.permissionRequest'),
      };
    }

    const displayTitle =
      optionalDisplayText(tool_call.title) ||
      optionalDisplayText(tool_call.raw_input?.description) ||
      t('messages.permissionRequest');

    return {
      title: displayTitle,
    };
  };
  const { title } = getToolInfo();
  const [selected, setSelected] = useState<string | null>(null);
  const [isResponding, setIsResponding] = useState(false);
  const [hasResponded, setHasResponded] = useState(false);
  const rawKind = optionalDisplayText(tool_call?.kind);
  const kind = kindFromPermissionAction(rawKind === 'execute' ? 'exec' : rawKind);
  const approvalOptions =
    !readOnly && !hasResponded
      ? (options ?? []).map((option, index) => {
          const optionName = optionalDisplayText(option?.name) || `${t('messages.option')} ${index + 1}`;
          const option_id = optionalDisplayText(option?.option_id) || `option_${index}`;
          const kind = optionalDisplayText(option?.kind);
          const translationKey =
            kind && isAcpPermissionOptionKind(kind) ? ACP_PERMISSION_OPTION_I18N_KEYS[kind] : undefined;
          const label = translationKey ? t(translationKey, { defaultValue: optionName }) : optionName;
          return { id: option_id, label };
        })
      : [];

  const handleConfirm = async () => {
    if (readOnly || hasResponded || !selected) return;

    setIsResponding(true);
    try {
      const toolCallId = parseConfirmationCorrelationId(tool_call.tool_call_id);
      const invokeData = {
        confirm_key: selected,
        msg_id: message.msg_id ?? toolCallId,
        conversation_id: message.conversation_id,
        call_id: toolCallId,
      };

      await conversation.confirmMessage.invoke(invokeData);
      setHasResponded(true);
    } catch (error) {
      // Handle error case - could add error logging here
      console.error('Error confirming permission:', error);
    } finally {
      setIsResponding(false);
    }
  };

  if (!tool_call) {
    return null;
  }

  return (
    <div className='mb-4' data-testid='message-acp-permission-card'>
      <ApprovalCard
        title={title}
        kind={kind}
        options={approvalOptions}
        selectedId={selected}
        onSelect={setSelected}
        onConfirm={() => {
          void handleConfirm();
        }}
        confirmLabel={isResponding ? t('messages.processing') : t('messages.confirm')}
        disabled={readOnly || isResponding || hasResponded}
      >
        {tool_call.raw_input?.command || tool_call.title ? (
          <div>
            <Text className='text-xs text-t-secondary mb-1'>{t('messages.command')}</Text>
            <code className='text-xs bg-1 p-2 rounded block text-t-primary break-all'>
              {toDisplayText(tool_call.raw_input?.command || tool_call.title)}
            </code>
          </div>
        ) : null}
      </ApprovalCard>
      {hasResponded && (
        <div
          className='mt-10px p-2 rounded-md border'
          style={{ backgroundColor: 'var(--color-success-light-1)', borderColor: 'rgb(var(--success-3))' }}
        >
          <Text className='text-sm' style={{ color: 'rgb(var(--success-6))' }}>
            ✓ {t('messages.responseSentSuccessfully')}
          </Text>
        </div>
      )}
    </div>
  );
});

export default MessageAcpPermission;
