/**
 * Sync open-ai-canvas config store models from allo catalogs.
 * - Image / video / TTS: `/api/media/models` (Flowy category 6 / 4 / 8)
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
import { FLOWY_CLOUD_CHANNEL_NAME, rewriteCatalogIconUrl } from './catalogIcon';

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
  models: Array<{ id: string; name?: string; icon?: string; supportsVision?: boolean }>,
  capability: ModelCapability,
  serverBaseUrl?: string
): NonNullable<ModelChannel['modelCosts']> {
  return models
    .map((m) => {
      const model = (m.id || m.name || '').trim();
      if (!model) return null;
      const icon = rewriteCatalogIconUrl(m.icon, serverBaseUrl);
      return {
        model,
        displayName: (m.name || m.id || model).trim(),
        ...(icon ? { icon } : {}),
        capability,
        billingMode: 'fixed_request' as const,
        unitPriceMicrocredits: 0,
        ...(capability === 'text' ? { protocol: 'chat-completion' as const } : {}),
        ...(capability === 'audio' ? { protocol: 'openai-audio' as const } : {}),
        ...(m.supportsVision ? { supportsVision: true } : {}),
      };
    })
    .filter((item): item is NonNullable<typeof item> => Boolean(item));
}

function catalogIconFromProviderDetail(
  providers: Array<{
    id: string;
    models_detail?: Array<{ model: string; params?: unknown; traits?: string[] }>;
  }>,
  providerId: string,
  modelId: string
): string | undefined {
  const detail = providers
    .find((p) => p.id === providerId)
    ?.models_detail?.find((row) => row.model === modelId);
  const params = detail?.params;
  if (!params || typeof params !== 'object' || Array.isArray(params)) return undefined;
  const icon = (params as Record<string, unknown>).icon
    ?? (params as Record<string, unknown>)._flowy_catalog_icon;
  return typeof icon === 'string' ? icon : undefined;
}

function catalogSupportsVision(
  providers: Array<{
    id: string;
    models_detail?: Array<{ model: string; traits?: string[]; params?: unknown }>;
  }>,
  providerId: string,
  modelId: string
): boolean {
  const detail = providers
    .find((p) => p.id === providerId)
    ?.models_detail?.find((row) => row.model === modelId);
  const traits = detail?.traits ?? [];
  if (traits.some((trait) => trait === 'vision_input')) return true;
  const params = detail?.params;
  if (!params || typeof params !== 'object' || Array.isArray(params)) return false;
  const extra = (params as Record<string, unknown>).extra;
  const parsed =
    typeof extra === 'string'
      ? (() => {
          try {
            return JSON.parse(extra) as { input?: unknown };
          } catch {
            return null;
          }
        })()
      : extra && typeof extra === 'object'
        ? (extra as { input?: unknown })
        : null;
  const input = parsed?.input;
  return Array.isArray(input) && input.some((item) => String(item).toLowerCase() === 'image');
}

async function fetchAlloChatModels(): Promise<Array<{ id: string; name: string; icon?: string; supportsVision?: boolean }>> {
  const [resolved, providers] = await Promise.all([
    ipcBridge.modelProfile.resolve.invoke({ task: 'chat' }),
    ipcBridge.mode.listProviders.invoke(),
  ]);
  const refs = resolved?.models ?? [];
  const labelByProvider = new Map(
    (providers ?? []).map((p) => [p.id, p.name || p.id] as const)
  );
  const seen = new Set<string>();
  const models: Array<{ id: string; name: string; icon?: string; supportsVision?: boolean }> = [];
  for (const ref of refs) {
    const id = (ref.model || '').trim();
    if (!id || seen.has(id)) continue;
    seen.add(id);
    const provider = (providers ?? []).find((p) => p.id === ref.provider_id);
    const desc = provider?.model_descriptions?.[id];
    const providerLabel = labelByProvider.get(ref.provider_id);
    const icon = catalogIconFromProviderDetail(providers ?? [], ref.provider_id, id);
    models.push({
      id,
      name: (desc || (providerLabel ? `${providerLabel} · ${id}` : id)).trim(),
      ...(icon ? { icon } : {}),
      ...(catalogSupportsVision(providers ?? [], ref.provider_id, id) ? { supportsVision: true } : {}),
    });
  }
  return models;
}

export type AlloCatalogModels = {
  image: IMediaModelOption[];
  video: IMediaModelOption[];
  audio: IMediaModelOption[];
  chat: Array<{ id: string; name: string; icon?: string; supportsVision?: boolean }>;
  /** Flowy API origin used to resolve relative catalog `icon` paths. */
  serverBaseUrl?: string;
};

/** Merge Flowy / local catalogs into a canvas `AiConfig`. Returns null when nothing to apply. */
export function mergeAlloCatalogIntoConfig(
  config: AiConfig,
  catalog: AlloCatalogModels
): AiConfig | null {
  const imageModels = catalog.image;
  const videoModels = catalog.video;
  const audioModels = catalog.audio;
  const chatModels = catalog.chat;
  const serverBaseUrl = catalog.serverBaseUrl;
  if (!imageModels.length && !videoModels.length && !audioModels.length && !chatModels.length) {
    return null;
  }

  const imageOptions = encodeOptions(imageModels, ALLO_MEDIA_CHANNEL_ID);
  const videoOptions = encodeOptions(videoModels, ALLO_MEDIA_CHANNEL_ID);
  const audioOptions = encodeOptions(audioModels, ALLO_MEDIA_CHANNEL_ID);
  const mediaRawIds = unique([
    ...imageModels.map((m) => m.id || m.name),
    ...videoModels.map((m) => m.id || m.name),
    ...audioModels.map((m) => m.id || m.name),
  ]);
  const mediaCosts = [
    ...costEntries(
      imageModels.map((m) => ({ id: m.id || m.name, name: m.name, icon: m.icon })),
      'image',
      serverBaseUrl
    ),
    ...costEntries(
      videoModels.map((m) => ({ id: m.id || m.name, name: m.name, icon: m.icon })),
      'video',
      serverBaseUrl
    ),
    ...costEntries(
      audioModels.map((m) => ({ id: m.id || m.name, name: m.name, icon: m.icon })),
      'audio',
      serverBaseUrl
    ),
  ];

  const alloMediaChannel = createModelChannel({
    id: ALLO_MEDIA_CHANNEL_ID,
    name: FLOWY_CLOUD_CHANNEL_NAME,
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
    name: FLOWY_CLOUD_CHANNEL_NAME,
    models: chatRawIds,
    baseUrl: ALLO_CHAT_LLM_BASE_URL,
    apiKey: 'system',
    apiFormat: 'openai',
    interfaceType: 'chat-completion',
    scope: 'system',
    enabled: true,
    hasApiKey: true,
    modelCosts: costEntries(chatModels, 'text', serverBaseUrl),
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
    models: unique([...textOptions, ...imageOptions, ...videoOptions, ...audioOptions]),
    imageModels: imageOptions,
    videoModels: videoOptions,
    audioModels: audioOptions,
    textModels: textOptions,
    imageModel: imageOptions[0] || config.imageModel,
    videoModel: videoOptions[0] || config.videoModel,
    audioModel: audioOptions[0] || config.audioModel,
    textModel: textOptions[0] || config.textModel,
    model: imageOptions[0] || videoOptions[0] || audioOptions[0] || textOptions[0] || config.model,
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

  return {
    ...normalized.config,
    channels,
    models: unique([
      ...textOptions,
      ...imageOptions,
      ...videoOptions,
      ...audioOptions,
      ...keepForeignModels,
    ]),
    imageModels: imageOptions.length ? imageOptions : normalized.config.imageModels,
    videoModels: videoOptions.length ? videoOptions : normalized.config.videoModels,
    audioModels: audioOptions.length ? audioOptions : normalized.config.audioModels,
    textModels: textOptions.length ? textOptions : normalized.config.textModels,
    imageModel: imageOptions.includes(normalized.config.imageModel)
      ? normalized.config.imageModel
      : imageOptions[0] || normalized.config.imageModel,
    videoModel: videoOptions.includes(normalized.config.videoModel)
      ? normalized.config.videoModel
      : videoOptions[0] || normalized.config.videoModel,
    audioModel: audioOptions.includes(normalized.config.audioModel)
      ? normalized.config.audioModel
      : audioOptions[0] || normalized.config.audioModel,
    textModel: preferredText,
    model:
      imageOptions[0] ||
      videoOptions[0] ||
      audioOptions[0] ||
      preferredText ||
      normalized.config.model,
  };
}

export async function syncOcConfigFromAlloMediaModels(): Promise<void> {
  const [mediaList, chatModels, whoami] = await Promise.all([
    fetchMediaModels().catch(() => ({ image_models: [], video_models: [], audio_models: [] })),
    fetchAlloChatModels().catch((err) => {
      console.warn('[videoCanvas] fetchAlloChatModels failed', err);
      return [] as Array<{ id: string; name: string; icon?: string; supportsVision?: boolean }>;
    }),
    ipcBridge.cloud.whoami.invoke().catch(() => null),
  ]);

  const merged = mergeAlloCatalogIntoConfig(useConfigStore.getState().config, {
    image: mediaList.image_models || [],
    video: mediaList.video_models || [],
    audio: mediaList.audio_models || [],
    chat: chatModels,
    serverBaseUrl: whoami?.serverBaseUrl,
  });
  if (!merged) return;

  useConfigStore.getState().replaceConfig(merged);
}
