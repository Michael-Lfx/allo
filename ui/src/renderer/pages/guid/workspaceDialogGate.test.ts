import { describe, expect, test } from 'bun:test';
import { createWorkspaceDialogGate } from './workspaceDialogGate';

describe('workspace dialog gate', () => {
  test('shares a pending native workspace selection instead of opening a second dialog', async () => {
    let resolveSelection!: () => void;
    let selections = 0;
    const selectWorkspace = () => {
      selections += 1;
      return new Promise<void>((resolve) => {
        resolveSelection = resolve;
      });
    };
    const openWorkspaceDialog = createWorkspaceDialogGate();

    const first = openWorkspaceDialog(selectWorkspace);
    const second = openWorkspaceDialog(selectWorkspace);

    expect(selections).toBe(1);

    resolveSelection();
    await Promise.all([first, second]);

    await openWorkspaceDialog(async () => {
      selections += 1;
    });
    expect(selections).toBe(2);
  });
});
