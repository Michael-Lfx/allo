import { describe, expect, test } from 'bun:test';
import type { ComposerSkillChip } from '@/renderer/components/chat/ComposerSkillChips';
import { appendComposerSkillChip, removeComposerSkillChip } from './useComposerSkillChips';

const pdf: ComposerSkillChip = { skillId: 'user:pdf', name: 'pdf', source: 'User' };
const projectPdf: ComposerSkillChip = { skillId: 'project:workspace:pdf', name: 'pdf', source: 'Project' };

describe('composer Skill chips', () => {
  test('deduplicates only the same source-qualified Skill ID', () => {
    expect(appendComposerSkillChip([pdf], pdf)).toEqual([pdf]);
    expect(appendComposerSkillChip([pdf], projectPdf)).toEqual([pdf, projectPdf]);
  });

  test('removes only the requested Skill chip', () => {
    expect(removeComposerSkillChip([pdf, projectPdf], pdf.skillId)).toEqual([projectPdf]);
  });
});
