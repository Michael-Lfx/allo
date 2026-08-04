import type { ComposerSkillChip } from '@/renderer/components/chat/composerSkill';
import { useCallback, useState } from 'react';

export function useComposerSkillChips(initialSkills: ComposerSkillChip[] = []) {
  const [skills, setSkills] = useState<ComposerSkillChip[]>(initialSkills);

  const clearSkills = useCallback(() => {
    setSkills([]);
  }, []);

  return { skills, clearSkills, setSkills };
}
