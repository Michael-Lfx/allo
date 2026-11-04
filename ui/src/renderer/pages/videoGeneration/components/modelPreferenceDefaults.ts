import type { IMediaModelOption } from '@/common/adapter/ipcBridge';
import { formatCloudModelLabel } from '@/renderer/utils/model/cloudModelLabel';
import { isWan3VideoModel } from '@renderer/services/videoModelCapabilities';

/** Preferred planning LLM catalog name (match id or display label). */
const PREFERRED_LLM_MODEL_NAME = 'Deepseek-v4-pro';

/** Image catalog: Seedream 5.0 Lite and Pro (match catalog `name`, fall back to id). */
const ALLOWED_IMAGE_MODEL_NAMES = [
  'Doubao-seedream-5-0-lite',
  'Doubao-seedream-5-0-pro',
] as const;

function normalizeModelKey(id: string): string {
  return id.toLowerCase().replace(/[^a-z0-9]/g, '');
}

/** Pick Wan 3.0 when listed; otherwise the first catalog entry. Prime is the Wan fallback. */
export function pickDefaultVideoModel(videoModels: IMediaModelOption[]): string | undefined {
  if (!videoModels.length) return undefined;
  const wan = videoModels.filter((m) => isWan3VideoModel(`${m.name} ${m.id}`));
  const standard = wan.find((m) => !normalizeModelKey(`${m.name} ${m.id}`).includes('prime'));
  return (standard ?? wan[0] ?? videoModels[0])?.id;
}

/** Prefer Deepseek-v4-pro by catalog name/id; otherwise the first chat model. */
export function pickDefaultLlmModel(modelIds: string[]): string | undefined {
  if (!modelIds.length) return undefined;
  const preferredKey = normalizeModelKey(PREFERRED_LLM_MODEL_NAME);
  const preferred = modelIds.find((id) => {
    const blob = normalizeModelKey(`${id} ${formatCloudModelLabel(id)}`);
    return blob.includes(preferredKey);
  });
  return preferred ?? modelIds[0];
}

/** Keep Seedream 5.0 Lite / Pro entries (match catalog `name`, fall back to id). */
export function filterAllowedImageModels(imageModels: IMediaModelOption[]): IMediaModelOption[] {
  const needles = ALLOWED_IMAGE_MODEL_NAMES.map(normalizeModelKey);
  return imageModels.filter((m) => {
    const nameKey = normalizeModelKey(m.name);
    const idKey = normalizeModelKey(m.id);
    return needles.some(
      (needle) => nameKey === needle || nameKey.includes(needle) || idKey.includes(needle)
    );
  });
}
