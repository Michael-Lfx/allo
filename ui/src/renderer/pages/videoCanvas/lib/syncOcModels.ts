/**
 * Sync open-ai-canvas config store models from allo /api/media/models.
 * Image / video lists stay capability-tagged so ModelPicker can filter them.
 */

import { fetchMediaModels } from '@renderer/hooks/agent/useMediaModels';
import type { IMediaModelOption } from '@/common/adapter/ipcBridge';
import {
  createModelChannel,
  encodeChannelModel,
  modelOptionName,
  normalizeConfigSnapshot,
  useConfigStore,
  type AiConfig,
  type ModelCapability,
  type ModelChannel,
} from '@oc/stores/use-config-store';

const ALLO_MEDIA_CHANNEL_ID = 'allo-media';

function unique(values: string[]) {
  return [...new Set(values.filter(Boolean))];
}

function encodeOptions(models: IMediaModelOption[], channelId: string) {
  return unique(models.map((m) => encodeChannelModel(channelId, m.id || m.name)));
}

function costEntries(
  models: IMediaModelOption[],
  capability: ModelCapability
): NonNullable<ModelChannel['modelCosts']> {
  return models
    .map((m) => {
      const model = (m.id || m.name || '').trim();
      if (!model) return null;
      return {
        model,
        displayName: (m.name || m.id || model).trim(),
        capability,
        billingMode: 'fixed_request' as const,
        unitPriceMicrocredits: 0,
      };
    })
    .filter((item): item is NonNullable<typeof item> => Boolean(item));
}

export async function syncOcConfigFromAlloMediaModels(): Promise<void> {
  const list = await fetchMediaModels();
  const imageModels = list.image_models || [];
  const videoModels = list.video_models || [];
  if (!imageModels.length && !videoModels.length) return;

  const store = useConfigStore.getState();
  const config = store.config;
  const channelId = ALLO_MEDIA_CHANNEL_ID;
  const rawModelIds = unique([
    ...imageModels.map((m) => m.id || m.name),
    ...videoModels.map((m) => m.id || m.name),
  ]);
  const imageOptions = encodeOptions(imageModels, channelId);
  const videoOptions = encodeOptions(videoModels, channelId);
  const modelCosts = [
    ...costEntries(imageModels, 'image'),
    ...costEntries(videoModels, 'video'),
  ];

  const alloChannel = createModelChannel({
    id: channelId,
    name: 'Allo Media',
    models: rawModelIds,
    baseUrl: config.baseUrl || 'http://127.0.0.1',
    apiKey: config.apiKey || 'system',
    apiFormat: config.apiFormat,
    scope: 'system',
    enabled: true,
    hasApiKey: true,
    modelCosts,
  });

  const otherChannels = (config.channels || []).filter((c) => c.id !== channelId);
  const channels = [alloChannel, ...otherChannels];

  const next: Partial<AiConfig> = {
    ...config,
    channels,
    models: [...imageOptions, ...videoOptions],
    imageModels: imageOptions,
    videoModels: videoOptions,
    imageModel: imageOptions[0] || config.imageModel,
    videoModel: videoOptions[0] || config.videoModel,
    model: imageOptions[0] || videoOptions[0] || config.model,
  };

  // Keep allo-tagged image/video lists after normalize (heuristic rebuild can drop unknown ids).
  const normalized = normalizeConfigSnapshot({ config: next });
  const merged: AiConfig = {
    ...normalized.config,
    channels,
    models: unique([
      ...imageOptions,
      ...videoOptions,
      ...(normalized.config.models || []).filter(
        (m) => modelOptionName(m) && !rawModelIds.includes(modelOptionName(m))
      ),
    ]),
    imageModels: imageOptions.length ? imageOptions : normalized.config.imageModels,
    videoModels: videoOptions.length ? videoOptions : normalized.config.videoModels,
    imageModel: imageOptions.includes(normalized.config.imageModel)
      ? normalized.config.imageModel
      : imageOptions[0] || normalized.config.imageModel,
    videoModel: videoOptions.includes(normalized.config.videoModel)
      ? normalized.config.videoModel
      : videoOptions[0] || normalized.config.videoModel,
    model:
      imageOptions[0] ||
      videoOptions[0] ||
      normalized.config.model,
  };

  store.replaceConfig(merged);
}
