import { CheckSmall } from '@icon-park/react';
import React from 'react';
import { useTranslation } from 'react-i18next';
import type { SkillCatalogEntry } from '@/renderer/hooks/skills/useSkillCatalog';
import SkillAvatar from '@/renderer/pages/settings/skill/SkillAvatar';
import { resolveSkillDisplay } from '@/renderer/pages/settings/skill/skillDisplay';
import styles from '../index.module.css';

export type DrawerSkillCardProps = {
  skill: SkillCatalogEntry;
  selected: boolean;
  onToggle: (skillId: string) => void;
};

const DrawerSkillCard: React.FC<DrawerSkillCardProps> = ({ skill, selected, onToggle }) => {
  const { t, i18n } = useTranslation();
  const display = resolveSkillDisplay(
    {
      name: skill.name,
      description: skill.description,
      name_i18n: skill.nameI18n,
      description_i18n: skill.descriptionI18n,
    },
    i18n.language || 'en-US'
  );

  return (
    <button
      type='button'
      className={[styles.drawerSkillCard, selected ? styles.drawerSkillCardSelected : ''].filter(Boolean).join(' ')}
      aria-pressed={selected}
      data-testid={`drawer-skill-${skill.skillId}`}
      onClick={() => onToggle(skill.skillId)}
    >
      <span className={styles.drawerSkillIcon} aria-hidden='true'>
        <SkillAvatar
          skill={{ name: skill.name, avatar: skill.avatar }}
          size={32}
          radiusClassName='rounded-9px'
          showShadow={false}
        />
      </span>
      <span className={styles.drawerSkillBody}>
        <span className={styles.drawerSkillTitleRow}>
          <span className={styles.drawerSkillTitle}>{display.name}</span>
          <span className={styles.drawerSkillSource}>
            {t(`conversation.skills.sources.${skill.source}`, { defaultValue: skill.source })}
          </span>
        </span>
        <span className={styles.drawerSkillDescription}>
          {display.description || t('guid.drawer.skillNoDescription', { defaultValue: '暂无描述。' })}
        </span>
      </span>
      <span className={[styles.drawerSkillStatus, selected ? styles.drawerSkillStatusSelected : ''].filter(Boolean).join(' ')} aria-hidden='true'>
        {selected ? <CheckSmall theme='filled' size={13} fill='currentColor' /> : null}
      </span>
    </button>
  );
};

export default DrawerSkillCard;
