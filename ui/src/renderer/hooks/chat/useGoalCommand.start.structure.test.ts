import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = () => readFileSync(new URL('./useGoalCommand.ts', import.meta.url), 'utf8');

describe('goal start action', () => {
  test('never sends the composer-only start action to the goal API', () => {
    const source = readSource();
    const startGuard = source.indexOf("if (invocation.action === 'start')");
    const goalApiCall = source.indexOf('ipcBridge.conversation.goalAction.invoke');

    expect(startGuard).toBeGreaterThan(-1);
    expect(goalApiCall).toBeGreaterThan(startGuard);
    expect(source.includes("if (invocation.action === 'start') {\n        return false;\n      }")).toBe(true);
  });
});
