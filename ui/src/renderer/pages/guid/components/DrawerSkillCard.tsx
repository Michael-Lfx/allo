import { CheckSmall, Puzzle } from '@icon-park/react';
import React from 'react';
import { useTranslation } from 'react-i18next';
import type { SkillCatalogEntry } from '@/renderer/hooks/skills/useSkillCatalog';
import styles from '../index.module.css';

export type DrawerSkillCardProps = {
  skill: SkillCatalogEntry;
  selected: boolean;
  onToggle: (skillId: string) => void;
};

const DrawerSkillCard: React.FC<DrawerSkillCardProps> = ({ skill, selected, onToggle }) => {
  const { t } = useTranslation();

  return (
    <button
      type='button'
      className={[styles.drawerSkillCard, selected ? styles.drawerSkillCardSelected : ''].filter(Boolean).join(' ')}
      aria-pressed={selected}
      data-testid={`drawer-skill-${skill.skillId}`}
      onClick={() => onToggle(skill.skillId)}
    >
      <span className={styles.drawerSkillIcon} aria-hidden='true'>
        <Puzzle theme='outline' size={16} fill='currentColor' />
      </span>
      <span className={styles.drawerSkillBody}>
        <span className={styles.drawerSkillTitleRow}>
          <span className={styles.drawerSkillTitle}>{skill.name}</span>
          <span className={styles.drawerSkillSource}>
            {t(`conversation.skills.sources.${skill.source}`, { defaultValue: skill.source })}
          </span>
        </span>
        <span className={styles.drawerSkillDescription}>
          {skill.description || t('guid.drawer.skillNoDescription', { defaultValue: '暂无描述。' })}
        </span>
      </span>
      <span className={[styles.drawerSkillStatus, selected ? styles.drawerSkillStatusSelected : ''].filter(Boolean).join(' ')} aria-hidden='true'>
        {selected ? <CheckSmall theme='filled' size={13} fill='currentColor' /> : null}
      </span>
    </button>
  );
};

export default DrawerSkillCard;
