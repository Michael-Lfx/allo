import { useTranslation } from 'react-i18next';
import { BookOpen, FileText, VideoOne } from '@icon-park/react';
import { CanvasMenuRow } from '@oc/components/canvas/canvas-overlay';

export interface SlashSkillMenuProps {
  items: ReadonlyArray<{ id: string; label: string; description: string }>;
  selectedId: string;
  onSelect: (id: string) => void;
}

const AGENT_MODE_ICONS = [
  <VideoOne key='idea' size={14} />,
  <FileText key='script' size={14} />,
  <BookOpen key='novel' size={14} />,
];

/** "/"-slash popover listing agent Modes (idea / script / novel). */
export function SlashSkillMenu({ items, selectedId, onSelect }: SlashSkillMenuProps) {
  const { t } = useTranslation();

  return (
    <div
      role='listbox'
      aria-label={t('videoGeneration.create.modesMenuAria', {
        defaultValue: '选择 Mode',
      })}
    >
      {items.map((skill, index) => (
        <CanvasMenuRow
          key={skill.id}
          icon={AGENT_MODE_ICONS[index]}
          label={skill.label}
          detail={skill.description}
          active={selectedId === skill.id}
          onClick={() => onSelect(skill.id)}
        />
      ))}
    </div>
  );
}
