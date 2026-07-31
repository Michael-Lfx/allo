import type { ComposerSkillChip } from '@/renderer/components/chat/ComposerSkillChips';
import { useCallback, useState } from 'react';

export function appendComposerSkillChip(
  current: ComposerSkillChip[],
  next: ComposerSkillChip,
): ComposerSkillChip[] {
  return current.some((skill) => skill.skillId === next.skillId) ? current : [...current, next];
}

export function removeComposerSkillChip(current: ComposerSkillChip[], skillId: string): ComposerSkillChip[] {
  return current.filter((skill) => skill.skillId !== skillId);
}

export function useComposerSkillChips(initialSkills: ComposerSkillChip[] = []) {
  const [skills, setSkills] = useState<ComposerSkillChip[]>(initialSkills);

  const addSkill = useCallback((skill: ComposerSkillChip) => {
    setSkills((current) => appendComposerSkillChip(current, skill));
  }, []);

  const removeSkill = useCallback((skillId: string) => {
    setSkills((current) => removeComposerSkillChip(current, skillId));
  }, []);

  const clearSkills = useCallback(() => {
    setSkills([]);
  }, []);

  return { skills, addSkill, removeSkill, clearSkills, setSkills };
}
