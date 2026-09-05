import { useTranslation } from 'react-i18next';
import { BookOpen, FileText, Pic, Platte, VideoOne } from '@icon-park/react';
import { CanvasMenuRow } from '@oc/components/canvas/canvas-overlay';

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
          <VideoOne key='idea' size={14} />,
          <FileText key='script' size={14} />,
          <BookOpen key='novel' size={14} />,
        ]
      : null;

  return (
    <div
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
      {items.map((skill, index) => (
        <CanvasMenuRow
          key={skill.id}
          icon={
            icons?.[index] ??
            (skill.id === 'cinematic' ? <Pic size={14} /> : <Platte size={14} />)
          }
          label={skill.label}
          detail={skill.description}
          active={selectedId === skill.id}
          onClick={() => onSelect(skill.id)}
        />
      ))}
    </div>
  );
}
