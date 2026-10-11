import { describe, expect, test } from 'bun:test';
import type { LocalModelRunStatus } from '@common/types/localModel';
import { isModelRunning, resolveCardActions } from './actions';

describe('model store card actions', () => {
  test('uninstalled shows only the download button', () => {
    const actions = resolveCardActions('uninstalled');
    expect(actions.download).toBe(true);
    expect(actions.play).toBe(false);
    expect(actions.stop).toBe(false);
    expect(actions.uninstall).toBe(false);
  });

  test('downloading shows progress and cancel', () => {
    const actions = resolveCardActions('downloading');
    expect(actions.progress).toBe(true);
    expect(actions.cancel).toBe(true);
    expect(actions.download).toBe(false);
    expect(actions.play).toBe(false);
  });

  test('installed shows play plus launch config and uninstall menu entries', () => {
    const actions = resolveCardActions('installed');
    expect(actions.play).toBe(true);
    expect(actions.stop).toBe(false);
    expect(actions.launchConfig).toBe(true);
    expect(actions.uninstall).toBe(true);
  });

  for (const status of ['starting', 'running'] as LocalModelRunStatus[]) {
    test(`${status} shows stop`, () => {
      const actions = resolveCardActions(status);
      expect(actions.stop).toBe(true);
      expect(actions.play).toBe(false);
    });
  }

  test('an installed Houmo model can be removed but is not launched here', () => {
    const actions = resolveCardActions('installed', false);
    expect(actions.play).toBe(false);
    expect(actions.launchConfig).toBe(false);
    expect(actions.uninstall).toBe(true);
    expect(actions.download).toBe(false);
  });

  test('isModelRunning covers starting and running', () => {
    expect(isModelRunning('starting')).toBe(true);
    expect(isModelRunning('running')).toBe(true);
    expect(isModelRunning('installed')).toBe(false);
  });
});
