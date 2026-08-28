import type { IMediaModelOption } from '@/common/adapter/ipcBridge';
import { formatCloudModelLabel } from '@/renderer/utils/model/cloudModelLabel';

/** Preferred default video model when present in the Flowy catalog. */
const PREFERRED_VIDEO_MODEL_NEEDLE = 'doubao-seedance-2-0-fast';

/** Preferred planning LLM catalog name (match id or display label). */
const PREFERRED_LLM_MODEL_NAME = 'Deepseek-v4-pro';

/** Image catalog is restricted to Seedream 5.0 Lite by catalog `name`. */
const ALLOWED_IMAGE_MODEL_NAME = 'Doubao-seedream-5-0-lite';

function normalizeModelKey(id: string): string {
  return id.toLowerCase().replace(/[^a-z0-9]/g, '');
}

/** Pick Seedance 2.0 Fast when listed; otherwise the first catalog entry. */
export function pickDefaultVideoModel(videoModels: IMediaModelOption[]): string | undefined {
  if (!videoModels.length) return undefined;
  const preferredKey = normalizeModelKey(PREFERRED_VIDEO_MODEL_NEEDLE);
  const preferred = videoModels.find((m) => {
    const blob = normalizeModelKey(`${m.name} ${m.id}`);
    return blob.includes(preferredKey);
  });
  return preferred?.id ?? videoModels[0]?.id;
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

/** Keep only Seedream 5.0 Lite entries (match catalog `name`, fall back to id). */
export function filterAllowedImageModels(imageModels: IMediaModelOption[]): IMediaModelOption[] {
  const needle = normalizeModelKey(ALLOWED_IMAGE_MODEL_NAME);
  return imageModels.filter((m) => {
    const nameKey = normalizeModelKey(m.name);
    const idKey = normalizeModelKey(m.id);
    return nameKey === needle || nameKey.includes(needle) || idKey.includes(needle);
  });
}
