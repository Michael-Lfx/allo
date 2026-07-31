import { CloseSmall } from '@icon-park/react';
import React from 'react';
import { useTranslation } from 'react-i18next';

export interface ComposerSkillChip {
  skillId: string;
  name: string;
  source: string;
}

interface ComposerSkillChipsProps {
  skills: ComposerSkillChip[];
  disabled?: boolean;
  onRemove: (skillId: string) => void;
}

const ComposerSkillChips: React.FC<ComposerSkillChipsProps> = ({ skills, disabled = false, onRemove }) => {
  const { t } = useTranslation();
  if (skills.length === 0) {
    return null;
  }

  return (
    <div className='mb-8px flex flex-wrap gap-6px' aria-label={t('conversation.skills.selected', { defaultValue: 'Selected Skills' })}>
      {skills.map((skill) => (
        <div
          key={skill.skillId}
          className='flex h-28px max-w-full items-center gap-6px border border-solid border-[var(--color-border-2)] bg-fill-1 px-8px text-12px text-t-primary'
          style={{ borderRadius: 6 }}
        >
          <span className='min-w-0 truncate'>{skill.name}</span>
          <span className='shrink-0 text-t-tertiary'>{skill.source}</span>
          <button
            type='button'
            className='flex size-18px shrink-0 items-center justify-center border-none bg-transparent p-0 text-t-secondary hover:text-t-primary disabled:cursor-not-allowed disabled:opacity-50'
            aria-label={t('conversation.skills.remove', { defaultValue: 'Remove {{name}}', name: skill.name })}
            disabled={disabled}
            onClick={() => onRemove(skill.skillId)}
          >
            <CloseSmall theme='outline' size={14} />
          </button>
        </div>
      ))}
    </div>
  );
};

export default ComposerSkillChips;
