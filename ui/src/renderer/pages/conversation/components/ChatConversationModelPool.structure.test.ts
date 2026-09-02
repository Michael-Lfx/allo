import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const source = readFileSync(new URL('./ChatConversation.tsx', import.meta.url), 'utf8');

describe('Conversation lead model authority', () => {
  test('updates the lead model and collaboration pool atomically for selection and healing', () => {
    expect(
      source.match(/updates: \{ model: selected, execution_model_pool, execution_template_id: null \}/g),
    ).toHaveLength(2);
    expect(source.includes('updates: { model: selected }')).toBe(false);
  });

  test('does not cancel a turn as a side effect of model selection', () => {
    expect(source.includes('conversation.stop.invoke')).toBe(false);
    expect(source.includes('modelMutationCoordinator')).toBe(true);
    expect(source.includes('beginExplicitSelection')).toBe(true);
    expect(source.includes('modelMutationCoordinator.enqueue')).toBe(true);
  });

  test('defers automatic model and pool reconciliation until runtime authority is idle', () => {
    expect(source.includes("if (runtimeAuthority !== 'idle') return;")).toBe(true);
    expect(source.includes('runtimeAuthority,')).toBe(true);
    expect(source.includes("if (!(isBackendHttpError(error) && error.status === 409))")).toBe(true);
  });

  test('treats model-selection conflicts as unapplied without stopping the turn', () => {
    const selectionSource = readFileSync(
      new URL('../platforms/nomi/useNomiModelSelection.ts', import.meta.url),
      'utf8',
    );
    const acpSource = readFileSync(new URL('../../../hooks/agent/useAcpModelInfo.ts', import.meta.url), 'utf8');

    expect(selectionSource).toContain('error.status === 409');
    expect(selectionSource).not.toContain('conversation.stop.invoke');
    expect(acpSource).toContain('activeTurnConflict');
    expect(acpSource).toContain('if (!activeTurnConflict)');
  });
});
