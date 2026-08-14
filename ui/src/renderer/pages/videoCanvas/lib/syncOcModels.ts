/**
 * Sync open-ai-canvas config store models from allo catalogs.
 * - Image / video: `/api/media/models`
 * - Text (Agent): `modelProfile.resolve({ task: 'chat' })` + provider list
 */

import { ipcBridge } from '@/common';
import type { IMediaModelOption } from '@/common/adapter/ipcBridge';
import { fetchMediaModels } from '@renderer/hooks/agent/useMediaModels';
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
const ALLO_CHAT_CHANNEL_ID = 'allo-chat';
/** Same-origin Flowy chat proxy owned by nomifun-canvas (agent loop stays client-side). */
export const ALLO_CHAT_LLM_BASE_URL = '/api/video-canvas/llm/v1';

function unique(values: string[]) {
  return [...new Set(values.filter(Boolean))];
}

function encodeOptions(models: IMediaModelOption[], channelId: string) {
  return unique(models.map((m) => encodeChannelModel(channelId, m.id || m.name)));
}

function costEntries(
  models: Array<{ id: string; name?: string; icon?: string }>,
  capability: ModelCapability
): NonNullable<ModelChannel['modelCosts']> {
  return models
    .map((m) => {
      const model = (m.id || m.name || '').trim();
      if (!model) return null;
      const icon = m.icon?.trim();
      return {
        model,
        displayName: (m.name || m.id || model).trim(),
        ...(icon ? { icon } : {}),
        capability,
        billingMode: 'fixed_request' as const,
        unitPriceMicrocredits: 0,
        ...(capability === 'text' ? { protocol: 'chat-completion' as const } : {}),
      };
    })
    .filter((item): item is NonNullable<typeof item> => Boolean(item));
}

async function fetchAlloChatModels(): Promise<Array<{ id: string; name: string }>> {
  const [resolved, providers] = await Promise.all([
    ipcBridge.modelProfile.resolve.invoke({ task: 'chat' }),
    ipcBridge.mode.listProviders.invoke(),
  ]);
  const refs = resolved?.models ?? [];
  const labelByProvider = new Map(
    (providers ?? []).map((p) => [p.id, p.name || p.id] as const)
  );
  const seen = new Set<string>();
  const models: Array<{ id: string; name: string }> = [];
  for (const ref of refs) {
    const id = (ref.model || '').trim();
    if (!id || seen.has(id)) continue;
    seen.add(id);
    const desc = (providers ?? []).find((p) => p.id === ref.provider_id)?.model_descriptions?.[id];
    const providerLabel = labelByProvider.get(ref.provider_id);
    models.push({
      id,
      name: (desc || (providerLabel ? `${providerLabel} · ${id}` : id)).trim(),
    });
  }
  return models;
}

export async function syncOcConfigFromAlloMediaModels(): Promise<void> {
  const [mediaList, chatModels] = await Promise.all([
    fetchMediaModels().catch(() => ({ image_models: [], video_models: [] })),
    fetchAlloChatModels().catch((err) => {
      console.warn('[videoCanvas] fetchAlloChatModels failed', err);
      return [] as Array<{ id: string; name: string }>;
    }),
  ]);

  const imageModels = mediaList.image_models || [];
  const videoModels = mediaList.video_models || [];
  if (!imageModels.length && !videoModels.length && !chatModels.length) return;

  const store = useConfigStore.getState();
  const config = store.config;

  const imageOptions = encodeOptions(imageModels, ALLO_MEDIA_CHANNEL_ID);
  const videoOptions = encodeOptions(videoModels, ALLO_MEDIA_CHANNEL_ID);
  const mediaRawIds = unique([
    ...imageModels.map((m) => m.id || m.name),
    ...videoModels.map((m) => m.id || m.name),
  ]);
  const mediaCosts = [
    ...costEntries(
      imageModels.map((m) => ({ id: m.id || m.name, name: m.name, icon: m.icon })),
      'image'
    ),
    ...costEntries(
      videoModels.map((m) => ({ id: m.id || m.name, name: m.name, icon: m.icon })),
      'video'
    ),
  ];

  const alloMediaChannel = createModelChannel({
    id: ALLO_MEDIA_CHANNEL_ID,
    name: 'Allo Media',
    models: mediaRawIds,
    baseUrl: config.baseUrl || 'http://127.0.0.1',
    apiKey: 'system',
    apiFormat: config.apiFormat,
    scope: 'system',
    enabled: true,
    hasApiKey: true,
    modelCosts: mediaCosts,
  });

  const chatRawIds = unique(chatModels.map((m) => m.id));
  const textOptions = unique(chatModels.map((m) => encodeChannelModel(ALLO_CHAT_CHANNEL_ID, m.id)));
  const alloChatChannel = createModelChannel({
    id: ALLO_CHAT_CHANNEL_ID,
    name: 'Flowy Cloud',
    models: chatRawIds,
    baseUrl: ALLO_CHAT_LLM_BASE_URL,
    apiKey: 'system',
    apiFormat: 'openai',
    interfaceType: 'chat-completion',
    scope: 'system',
    enabled: true,
    hasApiKey: true,
    modelCosts: costEntries(chatModels, 'text'),
  });

  const otherChannels = (config.channels || []).filter(
    (c) => c.id !== ALLO_MEDIA_CHANNEL_ID && c.id !== ALLO_CHAT_CHANNEL_ID
  );
  const channels = [
    ...(chatRawIds.length ? [alloChatChannel] : []),
    ...(mediaRawIds.length ? [alloMediaChannel] : []),
    ...otherChannels,
  ];

  const next: Partial<AiConfig> = {
    ...config,
    channels,
    models: unique([...textOptions, ...imageOptions, ...videoOptions]),
    imageModels: imageOptions,
    videoModels: videoOptions,
    textModels: textOptions,
    imageModel: imageOptions[0] || config.imageModel,
    videoModel: videoOptions[0] || config.videoModel,
    textModel: textOptions[0] || config.textModel,
    model: imageOptions[0] || videoOptions[0] || textOptions[0] || config.model,
  };

  const normalized = normalizeConfigSnapshot({ config: next });
  const keepForeignModels = (normalized.config.models || []).filter((m) => {
    const raw = modelOptionName(m);
    return raw && !mediaRawIds.includes(raw) && !chatRawIds.includes(raw);
  });

  const preferredText =
    textOptions.includes(normalized.config.textModel)
      ? normalized.config.textModel
      : textOptions[0] || normalized.config.textModel;

  const merged: AiConfig = {
    ...normalized.config,
    channels,
    models: unique([...textOptions, ...imageOptions, ...videoOptions, ...keepForeignModels]),
    imageModels: imageOptions.length ? imageOptions : normalized.config.imageModels,
    videoModels: videoOptions.length ? videoOptions : normalized.config.videoModels,
    textModels: textOptions.length ? textOptions : normalized.config.textModels,
    imageModel: imageOptions.includes(normalized.config.imageModel)
      ? normalized.config.imageModel
      : imageOptions[0] || normalized.config.imageModel,
    videoModel: videoOptions.includes(normalized.config.videoModel)
      ? normalized.config.videoModel
      : videoOptions[0] || normalized.config.videoModel,
    textModel: preferredText,
    model:
      imageOptions[0] ||
      videoOptions[0] ||
      preferredText ||
      normalized.config.model,
  };

  store.replaceConfig(merged);
}
