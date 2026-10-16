import type { IMessageSkillLoad } from '@/common/chat/chatLib';
import { ipcBridge } from '@/common';
import { useConversationContextSafe } from '@/renderer/hooks/context/ConversationContext';
import { MagicHat, Pin } from '@icon-park/react';
import React, { useState } from 'react';
import { useTranslation } from 'react-i18next';

interface MessageSkillLoadProps {
  message: IMessageSkillLoad;
}

/** Immutable, per-turn record of the exact SKILL.md snapshot sent to the Agent. */
const MessageSkillLoad: React.FC<MessageSkillLoadProps> = ({ message }) => {
  const { t } = useTranslation();
  const conversation = useConversationContextSafe();
  const { skill_id, name, source, version_hash, content, invocation } = message.content;
  const sourceLabel = t(`conversation.skills.sources.${source}`, { defaultValue: source });
  const shortVersion = version_hash.slice(0, 12);
  const autoLoaded = invocation === 'model-auto';
  const [pinning, setPinning] = useState(false);

  const pinForSession = async () => {
    if (!conversation?.conversation_id || pinning) return;
    setPinning(true);
    try {
      await ipcBridge.conversation.setSkillPin.invoke({
        conversation_id: conversation.conversation_id,
        skill_id,
        pinned: true,
      });
    } catch (error) {
      console.warn('[skills] failed to pin auto-loaded skill', error);
    } finally {
      setPinning(false);
    }
  };

  return (
    <div
      className='w-full border border-solid border-[var(--color-border-2)] bg-fill-1 px-10px py-8px text-12px text-t-primary'
      style={{ borderRadius: 6 }}
      data-testid='message-skill-load'
    >
      <div className='flex min-w-0 items-center gap-6px'>
        <MagicHat theme='outline' size='16' className='shrink-0 text-t-secondary' />
        <span className='min-w-0 truncate font-medium'>
          {t('messages.skillLoad.loaded', { name, defaultValue: 'Loaded {{name}}' })}
        </span>
        <span className='shrink-0 text-t-tertiary'>{sourceLabel}</span>
        {autoLoaded && (
          <span
            className='shrink-0 rounded-[8px] bg-[rgba(var(--success-6),0.12)] px-5px py-0 text-10px text-[rgb(var(--success-6))]'
            data-testid='message-skill-load-auto'
          >
            {t('messages.skillLoad.auto', { defaultValue: 'Auto' })}
          </span>
        )}
        {autoLoaded && conversation?.conversation_id && !conversation.readOnly && (
          <div
            role='button'
            tabIndex={0}
            data-testid='message-skill-load-pin'
            className='ml-auto flex shrink-0 cursor-pointer items-center gap-4px text-11px text-t-secondary hover:text-t-primary'
            onClick={() => void pinForSession()}
            onKeyDown={(event) => {
              if (event.key === 'Enter' || event.key === ' ') {
                event.preventDefault();
                void pinForSession();
              }
            }}
          >
            <Pin theme='outline' size={12} />
            {t('messages.skillLoad.pin', { defaultValue: 'Pin for session' })}
          </div>
        )}
      </div>
      <div className='mt-4px flex min-w-0 items-center gap-6px text-11px text-t-tertiary'>
        <span className='min-w-0 truncate font-mono' title={skill_id}>
          {skill_id}
        </span>
        <span className='shrink-0 font-mono' title={version_hash}>
          {t('messages.skillLoad.version', { version: shortVersion, defaultValue: 'Version {{version}}' })}
        </span>
      </div>
      <details className='mt-6px'>
        <summary className='cursor-pointer select-none text-t-secondary hover:text-t-primary'>
          {t('messages.skillLoad.details', { defaultValue: 'View loaded instructions' })}
        </summary>
        <pre className='mt-6px max-h-360px overflow-auto whitespace-pre-wrap break-words bg-transparent font-mono text-11px leading-18px text-t-secondary'>
          {content}
        </pre>
      </details>
    </div>
  );
};

export default MessageSkillLoad;
