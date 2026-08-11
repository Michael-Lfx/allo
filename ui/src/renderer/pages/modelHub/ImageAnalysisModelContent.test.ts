import { describe, expect, test } from 'bun:test';
import { resolveAutomaticImageAnalysisModel } from './ImageAnalysisModelContent';
import type { TaskModelGroup } from '@/renderer/hooks/agent/useModelsForTask';

const group = (id: string, models: string[]): TaskModelGroup =>
  ({ provider: { id } as TaskModelGroup['provider'], models }) as TaskModelGroup;

describe('resolveAutomaticImageAnalysisModel', () => {
  test('prefers MiniMax-M3 regardless of provider order', () => {
    expect(
      resolveAutomaticImageAnalysisModel([
        group('provider-a', ['vision-first']),
        group('provider-b', ['MiniMax-M3']),
      ])
    ).toEqual({ providerId: 'provider-b', model: 'MiniMax-M3' });
  });

  test('falls back to the first vision model when MiniMax-M3 is absent', () => {
    expect(resolveAutomaticImageAnalysisModel([group('provider-a', ['vision-first', 'vision-second'])])).toEqual({
      providerId: 'provider-a',
      model: 'vision-first',
    });
  });

  test('returns null while no vision models are available', () => {
    expect(resolveAutomaticImageAnalysisModel([])).toBeNull();
  });
});
