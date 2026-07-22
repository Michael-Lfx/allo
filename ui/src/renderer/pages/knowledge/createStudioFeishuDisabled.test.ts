import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import {
  FEISHU_KNOWLEDGE_CREATION_ENABLED,
  canSubmitStudioSourceConfig,
  canSubmitStudioSourceType,
  normalizeStudioInitialKind,
} from './CreateStudio/sourceTypes';

const typeRailSource = readFileSync(new URL('./CreateStudio/TypeRail.tsx', import.meta.url), 'utf8');
const emptyStateSource = readFileSync(new URL('./KnowledgeEmptyState.tsx', import.meta.url), 'utf8');

describe('CreateStudio Feishu creation gate', () => {
  test('enables Feishu knowledge-space creation', () => {
    expect(FEISHU_KNOWLEDGE_CREATION_ENABLED).toBe(true);
  });

  test('keeps Feishu when preselected through a shortcut', () => {
    expect(normalizeStudioInitialKind('feishu')).toBe('feishu');
    expect(normalizeStudioInitialKind('web')).toBe('web');
    expect(normalizeStudioInitialKind(undefined)).toBe('blank');
  });

  test('allows submitting a Feishu source when creation is enabled', () => {
    expect(canSubmitStudioSourceType('feishu')).toBe(true);
    expect(canSubmitStudioSourceType('blank')).toBe(true);
    expect(canSubmitStudioSourceType('local')).toBe(true);
    expect(canSubmitStudioSourceType('web')).toBe(true);
    expect(canSubmitStudioSourceType('import')).toBe(true);
  });

  test('requires an explicit folder path before submitting a local-folder source', () => {
    expect(canSubmitStudioSourceConfig('local', {})).toEqual({
      ok: false,
      messageKey: 'knowledge.studio.localFolderRequired',
    });
    expect(canSubmitStudioSourceConfig('local', { rootPath: '   ' })).toEqual({
      ok: false,
      messageKey: 'knowledge.studio.localFolderRequired',
    });
    expect(canSubmitStudioSourceConfig('local', { rootPath: '/Users/muri/docs' })).toEqual({ ok: true });
    expect(canSubmitStudioSourceConfig('blank', {})).toEqual({ ok: true });
    expect(canSubmitStudioSourceConfig('feishu', {})).toEqual({ ok: true });
  });

  test('wires visible Feishu shortcuts to the creation flag', () => {
    expect(typeRailSource.includes('FEISHU_KNOWLEDGE_CREATION_ENABLED')).toBe(true);
    expect(typeRailSource.includes('disabled: !FEISHU_KNOWLEDGE_CREATION_ENABLED')).toBe(true);

    expect(emptyStateSource.includes('FEISHU_KNOWLEDGE_CREATION_ENABLED')).toBe(true);
    expect(emptyStateSource.includes('disabled: !FEISHU_KNOWLEDGE_CREATION_ENABLED')).toBe(true);
    expect(emptyStateSource.includes('if (!k.disabled) onCreate(k.key);')).toBe(true);
  });
});
