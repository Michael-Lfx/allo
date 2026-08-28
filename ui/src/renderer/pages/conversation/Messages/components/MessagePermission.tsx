
import { parseConfirmationCorrelationId, type IMessagePermission } from '@/common/chat/chatLib';
import { optionalDisplayText, toDisplayText } from '@/common/chat/displayText';
import { ipcBridge } from '@/common';
import { useConversationContextSafe } from '@/renderer/hooks/context/ConversationContext';
import type { I18nKey } from '@/renderer/services/i18n';
import ApprovalCard from '@renderer/components/beautifulUi/approvalCard/ApprovalCard';
import { kindFromPermissionAction } from '@renderer/components/beautifulUi/approvalCard/approvalCardModel';
import { Image, Typography } from '@arco-design/web-react';
import React, { useState } from 'react';
import { useTranslation } from 'react-i18next';

const { Text } = Typography;

const PERMISSION_OPTION_I18N_KEYS: Record<string, I18nKey> = {
  allow_once: 'messages.confirmation.yesAllowOnce',
  'allow-once': 'messages.confirmation.yesAllowOnce',
  'Allow once': 'messages.confirmation.yesAllowOnce',
  allow_always: 'messages.confirmation.yesAllowAlways',
  'allow-always': 'messages.confirmation.yesAllowAlways',
  'Allow always': 'messages.confirmation.yesAllowAlways',
  reject_once: 'messages.confirmation.rejectOnce',
  'reject-once': 'messages.confirmation.rejectOnce',
  'Reject once': 'messages.confirmation.rejectOnce',
  deny: 'messages.confirmation.rejectOnce',
  deny_once: 'messages.confirmation.rejectOnce',
  'deny-once': 'messages.confirmation.rejectOnce',
  reject_always: 'messages.confirmation.rejectAlways',
  'reject-always': 'messages.confirmation.rejectAlways',
  'Reject always': 'messages.confirmation.rejectAlways',
  deny_always: 'messages.confirmation.rejectAlways',
  'deny-always': 'messages.confirmation.rejectAlways',
};

interface MessagePermissionProps {
  message: IMessagePermission;
}

const MessagePermission: React.FC<MessagePermissionProps> = React.memo(({ message }) => {
  const { t } = useTranslation();
  const readOnly = useConversationContextSafe()?.readOnly === true;
  const { options = [], description, title, action, call_id, command_type, screenshot } = message.content || {};
  const descriptionText = optionalDisplayText(description);
  const titleText = optionalDisplayText(title);
  const actionText = optionalDisplayText(action);
  const commandTypeText = optionalDisplayText(command_type);
  const screenshotSrc = optionalDisplayText(screenshot);

  const [selected, setSelected] = useState<string | null>(null);
  const [isResponding, setIsResponding] = useState(false);
  const [hasResponded, setHasResponded] = useState(false);

  const displayTitle = titleText || descriptionText || t('messages.permissionRequest');
  const cardDescription = descriptionText && descriptionText !== displayTitle ? descriptionText : undefined;
  const approvalOptions =
    !readOnly && !hasResponded
      ? options.map((option, index) => {
          const optionLabel = toDisplayText(option.label);
          const translationKey = PERMISSION_OPTION_I18N_KEYS[optionLabel] ?? optionLabel;
          return {
            id: String(option.value) || `option_${index}`,
            label: t(translationKey, { ...option.params, defaultValue: optionLabel }),
          };
        })
      : [];

  const handleConfirm = async () => {
    if (readOnly || hasResponded || !selected) return;

    setIsResponding(true);
    try {
      const always_allow = selected === 'proceed_always';
      await ipcBridge.conversation.confirmation.confirm.invoke({
        conversation_id: message.conversation_id,
        call_id,
        msg_id: message.msg_id ?? parseConfirmationCorrelationId(message.content.id),
        data: { value: selected },
        always_allow,
      });
      setHasResponded(true);
    } catch (error) {
      console.error('Error confirming permission:', error);
    } finally {
      setIsResponding(false);
    }
  };

  return (
    <div className='mb-4' data-testid='message-permission-card'>
      <ApprovalCard
        title={displayTitle}
        kind={kindFromPermissionAction(actionText)}
        description={cardDescription}
        options={approvalOptions}
        selectedId={selected}
        onSelect={setSelected}
        onConfirm={() => {
          void handleConfirm();
        }}
        confirmLabel={isResponding ? t('messages.processing') : t('messages.confirm')}
        disabled={readOnly || isResponding || hasResponded}
      >
        {commandTypeText ? (
          <div>
            <Text className='text-xs text-t-secondary mb-1'>{t('messages.command')}</Text>
            <code className='text-xs bg-1 p-2 rounded block text-t-primary break-all'>{commandTypeText}</code>
          </div>
        ) : null}
        {screenshotSrc ? (
          <div className='rounded-md overflow-hidden border' style={{ borderColor: 'var(--border-2)' }}>
            <Image src={screenshotSrc} alt={t('messages.browserApprovalPreview')} width='100%' style={{ maxHeight: 320, objectFit: 'contain' }} />
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

export default MessagePermission;
