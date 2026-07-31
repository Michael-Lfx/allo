import type { IMessageSkillLoad } from '@/common/chat/chatLib';
import { MagicHat } from '@icon-park/react';
import React from 'react';
import { useTranslation } from 'react-i18next';

interface MessageSkillLoadProps {
  message: IMessageSkillLoad;
}

/** Immutable, per-turn record of the exact SKILL.md snapshot sent to the Agent. */
const MessageSkillLoad: React.FC<MessageSkillLoadProps> = ({ message }) => {
  const { t } = useTranslation();
  const { skill_id, name, source, version_hash, content } = message.content;
  const sourceLabel = t(`conversation.skills.sources.${source}`, { defaultValue: source });
  const shortVersion = version_hash.slice(0, 12);

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
