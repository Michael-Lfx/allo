import { describe, expect, test } from 'bun:test';
import type { IProvider } from '@/common/config/storage';
import { FLOWY_BUILTIN_PROVIDER_ID } from '@/common/types/ids';
import type { TaskModelGroup } from '@/renderer/hooks/agent/useModelsForTask';
import {
  allChatModelOptions,
  buildChatModelPickerViewModel,
  findChatModelOption,
} from './chatModelPicker';

const provider = (id: string, details: Array<Record<string, unknown>>): IProvider =>
  ({
    id,
    name: id,
    platform: 'openai',
    enabled: true,
    models: details.map((detail) => detail.model),
    models_detail: details,
  }) as unknown as IProvider;

const group = (providerValue: IProvider, models: string[]): TaskModelGroup => ({
  provider: providerValue,
  models,
});

describe('chat model picker view model', () => {
  test('groups Auto, Cloud, and other providers without model-name heuristics', () => {
    const flowy = provider(FLOWY_BUILTIN_PROVIDER_ID, [
      {
        model: 'AIPC-auto-cost',
        params: { _flowy_catalog_family: 'auto', _flowy_catalog_auto_tier: 'cost' },
        traits: ['function_calling'],
      },
      {
        model: 'AIPC-auto-intelligence',
        params: { _flowy_catalog_family: 'auto', _flowy_catalog_auto_tier: 'intelligence' },
        traits: ['function_calling'],
      },
      {
        model: 'AIPC-auto-balance',
        params: { _flowy_catalog_family: 'auto', _flowy_catalog_auto_tier: 'balance' },
        traits: ['function_calling'],
      },
      {
        model: 'AIPC-cloud',
        params: {
          _flowy_catalog_family: 'cloud',
          _flowy_catalog_reasoning_effort: ['low', 'medium', 'xhigh'],
          _flowy_catalog_credit_rate: 0.5,
        },
        traits: ['vision_input'],
      },
      {
        model: 'AIPC-auto-like-but-legacy',
        params: {},
        traits: [],
      },
    ]);
    const other = provider('custom-provider', [
      {
        model: 'AIPC-auto-cost',
        params: { _flowy_catalog_family: 'auto' },
        traits: [],
      },
    ]);

    const viewModel = buildChatModelPickerViewModel([
      group(flowy, [
        'AIPC-auto-cost',
        'AIPC-auto-intelligence',
        'AIPC-auto-balance',
        'AIPC-cloud',
        'AIPC-auto-like-but-legacy',
      ]),
      group(other, ['AIPC-auto-cost']),
    ]);

    expect(viewModel.autoModels.map((option) => option.autoTier)).toEqual([
      'intelligence',
      'balance',
      'cost',
    ]);
    expect(viewModel.cloudModels.map((option) => option.model)).toEqual([
      'AIPC-cloud',
      'AIPC-auto-like-but-legacy',
    ]);
    expect(viewModel.otherProviderGroups).toHaveLength(1);
    expect(viewModel.otherProviderGroups[0]?.provider.id).toBe('custom-provider');
    expect(viewModel.otherProviderGroups[0]?.models).toEqual(['AIPC-auto-cost']);
  });

  test('keeps Cloud effort metadata and disables text-only models when images are attached', () => {
    const flowy = provider(FLOWY_BUILTIN_PROVIDER_ID, [
      {
        model: 'AIPC-auto-balance',
        params: { _flowy_catalog_family: 'auto', _flowy_catalog_auto_tier: 'balance' },
        traits: ['function_calling'],
      },
      {
        model: 'AIPC-text-cloud',
        params: { _flowy_catalog_family: 'cloud', _flowy_catalog_reasoning_effort: ['low', 'medium', 'xhigh'] },
        traits: [],
      },
      {
        model: 'AIPC-vision-cloud',
        params: { _flowy_catalog_family: 'cloud', _flowy_catalog_reasoning_effort: ['medium'] },
        traits: ['vision_input'],
      },
    ]);
    const viewModel = buildChatModelPickerViewModel(
      [group(flowy, ['AIPC-auto-balance', 'AIPC-text-cloud', 'AIPC-vision-cloud'])],
      { hasImageAttachments: true }
    );

    expect(viewModel.autoModels[0]?.reasoningLevels).toEqual([]);
    expect(viewModel.autoModels[0]?.disabled).toBe(true);
    expect(viewModel.cloudModels[0]?.reasoningLevels).toEqual(['low', 'medium', 'xhigh']);
    expect(viewModel.cloudModels[0]?.disabled).toBe(true);
    expect(viewModel.cloudModels[1]?.disabled).toBe(false);
    expect(findChatModelOption(viewModel, FLOWY_BUILTIN_PROVIDER_ID, 'AIPC-auto-balance')?.model).toBe(
      'AIPC-auto-balance'
    );
  });

  test('recomputes attachment restrictions when a cached picker is reused', () => {
    const flowy = provider(FLOWY_BUILTIN_PROVIDER_ID, [
      { model: 'AIPC-auto-balance', params: { _flowy_catalog_family: 'auto' }, traits: [] },
      { model: 'AIPC-text-cloud', params: { _flowy_catalog_family: 'cloud' }, traits: [] },
      { model: 'AIPC-vision-cloud', params: { _flowy_catalog_family: 'cloud' }, traits: ['vision_input'] },
    ]);
    const viewModel = buildChatModelPickerViewModel([
      group(flowy, ['AIPC-auto-balance', 'AIPC-text-cloud', 'AIPC-vision-cloud']),
    ]);
    const withImages = allChatModelOptions(viewModel, { hasImageAttachments: true });

    expect(withImages.find((option) => option.model === 'AIPC-auto-balance')?.disabled).toBe(true);
    expect(withImages.find((option) => option.model === 'AIPC-text-cloud')?.disabled).toBe(true);
    expect(withImages.find((option) => option.model === 'AIPC-vision-cloud')?.disabled).toBe(false);
  });
});
