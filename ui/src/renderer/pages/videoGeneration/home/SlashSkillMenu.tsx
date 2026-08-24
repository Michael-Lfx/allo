import { useTranslation } from 'react-i18next';
import { BookOpen, FileText, Pic, Platte, VideoOne } from '@icon-park/react';
import styles from './home.module.css';

export interface SlashSkillMenuProps {
  mode: 'agent' | 'creation';
  items: ReadonlyArray<{ id: string; label: string; description: string }>;
  selectedId: string;
  onSelect: (id: string) => void;
}

/** "/"-slash popover listing agent Modes or creation style skills. */
export function SlashSkillMenu({ mode, items, selectedId, onSelect }: SlashSkillMenuProps) {
  const { t } = useTranslation();
  const icons =
    mode === 'agent'
      ? [
          <VideoOne key='idea' size={15} />,
          <FileText key='script' size={15} />,
          <BookOpen key='novel' size={15} />,
        ]
      : null;

  return (
    <div
      className={styles.slashMenu}
      role='listbox'
      aria-label={
        mode === 'agent'
          ? t('videoGeneration.create.modesMenuAria', {
              defaultValue: '选择 Mode',
            })
          : t('videoGeneration.create.skillsMenuAria', {
              defaultValue: '选择技能',
            })
      }
    >
      <div className={styles.slashMenuTitle}>
        {mode === 'agent'
          ? t('videoGeneration.create.modesMenuTitle', {
              defaultValue: '选择 Mode',
            })
          : t('videoGeneration.create.skillsMenuTitle', {
              defaultValue: '选择风格技能',
            })}
      </div>
      {items.map((skill, index) => {
        const active = selectedId === skill.id;
        return (
          <button
            key={skill.id}
            type='button'
            role='option'
            aria-selected={active}
            className={`${styles.slashMenuItem} ${
              active ? styles.slashMenuItemActive : ''
            }`}
            onClick={() => onSelect(skill.id)}
          >
            {icons?.[index] ??
              (skill.id === 'cinematic' ? <Pic size={15} /> : <Platte size={15} />)}
            <span>
              <strong>{skill.label}</strong>
              <small>{skill.description}</small>
            </span>
          </button>
        );
      })}
    </div>
  );
}
