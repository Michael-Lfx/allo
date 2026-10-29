import { describe, expect, test as it } from 'bun:test';
import { GUID_DRAFT_KEY, useComposerDraftStore } from '@/renderer/stores/composerDraftStore';

describe('Guid Draft Skills preservation and roundtrip (F4, CP-03)', () => {
  it('stores and restores complete skill chip metadata without field loss', () => {
    const sampleSkills = [
      {
        skillId: 'skill-alpha',
        name: 'Alpha Reviewer',
        source: 'custom',
        avatar: 'https://example.com/avatar.png',
      },
      {
        skillId: 'skill-beta',
        name: 'Beta Translator',
        source: 'builtin',
      },
    ];

    // Store skills into guid draft
    useComposerDraftStore.getState().setDraft(GUID_DRAFT_KEY, {
      text: 'Draft prompt text',
      files: ['/path/to/file.ts'],
      dir: '/workspace/project',
      skillChips: sampleSkills,
    });

    const retrieved = useComposerDraftStore.getState().drafts[GUID_DRAFT_KEY];
    expect(retrieved).toBeDefined();
    expect(retrieved?.text).toBe('Draft prompt text');
    expect(retrieved?.files).toEqual(['/path/to/file.ts']);
    expect(retrieved?.dir).toBe('/workspace/project');
    expect(retrieved?.skillChips).toEqual(sampleSkills);

    // Verify avatar and source are preserved, not stripped or cast away
    expect(retrieved?.skillChips?.[0]?.source).toBe('custom');
    expect(retrieved?.skillChips?.[0]?.avatar).toBe('https://example.com/avatar.png');
    expect(retrieved?.skillChips?.[1]?.source).toBe('builtin');
    expect(retrieved?.skillChips?.[1]?.avatar).toBeUndefined();
  });

  it('preserves skills when input text or directory is updated independently', () => {
    const existingSkills = [
      {
        skillId: 'skill-gamma',
        name: 'Gamma Helper',
        source: 'community',
      },
    ];

    useComposerDraftStore.getState().setDraft(GUID_DRAFT_KEY, {
      skillChips: existingSkills,
    });

    // Update only text
    useComposerDraftStore.getState().setDraft(GUID_DRAFT_KEY, {
      text: 'New text without overriding skills',
    });

    const draft = useComposerDraftStore.getState().drafts[GUID_DRAFT_KEY];
    expect(draft?.text).toBe('New text without overriding skills');
    expect(draft?.skillChips).toEqual(existingSkills);
  });
});
