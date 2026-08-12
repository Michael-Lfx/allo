import { describe, expect, test } from 'bun:test';
import { NOMIFUN_FREE_MODEL_PLATFORM } from '@/common/types/provider/managedModelService';
import { resolveAutomaticImageAnalysisModel } from './ImageAnalysisModelContent';
import type { TaskModelGroup } from '@/renderer/hooks/agent/useModelsForTask';

const group = (id: string, models: string[], platform = 'openai'): TaskModelGroup =>
  ({ provider: { id, platform } as TaskModelGroup['provider'], models }) as TaskModelGroup;

describe('resolveAutomaticImageAnalysisModel', () => {
  test('prefers MiniMax-M3 regardless of provider order', () => {
    expect(
      resolveAutomaticImageAnalysisModel([
        group('provider-a', ['vision-first']),
        group('provider-b', ['MiniMax-M3']),
      ])
    ).toEqual({ providerId: 'provider-b', model: 'MiniMax-M3' });
  });

  test('prefers Flowy Cloud AIPC-Minimax-M3 over earlier vision models', () => {
    expect(
      resolveAutomaticImageAnalysisModel([
        group('flowy-cloud', ['AIPC-GPT5.5', 'AIPC-Kimi-K2.6', 'AIPC-Minimax-M3']),
        group('free', ['mimo-v2.5-free']),
      ])
    ).toEqual({ providerId: 'flowy-cloud', model: 'AIPC-Minimax-M3' });
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

  test('ignores NomiFun free models even when they are the only vision option', () => {
    expect(
      resolveAutomaticImageAnalysisModel([
        group('free', ['mimo-v2.5-free'], NOMIFUN_FREE_MODEL_PLATFORM),
      ])
    ).toBeNull();
  });
});
