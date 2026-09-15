import { describe, expect, test } from 'bun:test';
import type { IProvider } from '@/common/config/storage';
import { FLOWY_BUILTIN_PROVIDER_ID } from '@/common/types/ids';
import type { TaskModelGroup } from '@/renderer/hooks/agent/useModelsForTask';
import {
  AUTO_TIER_LABEL_FALLBACK,
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
  test('keeps the user-facing Auto tier fallback labels stable', () => {
    expect(AUTO_TIER_LABEL_FALLBACK).toEqual({
      intelligence: 'Smart',
      balance: 'Balanced',
      cost: 'Economy',
    });
  });

  test('groups Auto, Cloud, and other providers without model-name heuristics', () => {
    const flowy = provider(FLOWY_BUILTIN_PROVIDER_ID, [
      {
        model: 'AIPC-auto-cost',
        params: {
          _flowy_catalog_family: 'auto',
          _flowy_catalog_auto_tier: 'cost',
          _flowy_catalog_reasoning_effort: ['low', 'medium', 'xhigh'],
        },
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
    expect(viewModel.autoModels.find((option) => option.autoTier === 'cost')?.reasoningLevels).toEqual([]);
    expect(viewModel.cloudModels.map((option) => option.model)).toEqual([
      'AIPC-cloud',
      'AIPC-auto-like-but-legacy',
    ]);
    expect(viewModel.otherProviderGroups).toHaveLength(1);
    expect(viewModel.otherProviderGroups[0]?.provider.id).toBe('custom-provider');
    expect(viewModel.otherProviderGroups[0]?.models).toEqual(['AIPC-auto-cost']);
  });

  test('keeps Cloud effort metadata and never disables models over image attachments', () => {
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
      [group(flowy, ['AIPC-auto-balance', 'AIPC-text-cloud', 'AIPC-vision-cloud'])]
    );

    expect(viewModel.autoModels[0]?.reasoningLevels).toEqual([]);
    expect(viewModel.cloudModels[0]?.reasoningLevels).toEqual(['low', 'medium', 'xhigh']);
    // Image-bearing sends are handled by the backend image-analysis self-healing
    // chain; the picker must stay agnostic and leave every model selectable.
    expect(
      allChatModelOptions(viewModel).every((option) => !('disabled' in option))
    ).toBe(true);
    expect(findChatModelOption(viewModel, FLOWY_BUILTIN_PROVIDER_ID, 'AIPC-auto-balance')?.model).toBe(
      'AIPC-auto-balance'
    );
  });

  test('preserves normalized Auto metadata when provider details are temporarily unavailable', () => {
    const completeProvider = provider(FLOWY_BUILTIN_PROVIDER_ID, [
      {
        model: 'AIPC-auto-balance',
        params: { _flowy_catalog_family: 'auto', _flowy_catalog_auto_tier: 'balance' },
        traits: ['function_calling'],
      },
    ]);
    const normalized = buildChatModelPickerViewModel([
      group(completeProvider, ['AIPC-auto-balance']),
    ]).autoModels[0];
    const incompleteProvider = { ...completeProvider, models_detail: undefined } as IProvider;
    const viewModel = {
      autoModels: [{ ...normalized, provider: incompleteProvider }],
      cloudModels: [],
      otherProviderGroups: [],
    };

    const options = allChatModelOptions(viewModel);

    expect(options[0]).toMatchObject({
      family: 'auto',
      autoTier: 'balance',
      reasoningLevels: [],
      supportsTools: true,
    });
  });
});
