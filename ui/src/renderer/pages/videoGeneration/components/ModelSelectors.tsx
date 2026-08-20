/**
 * Per-session Flowy model pickers for video generation:
 * - LLM (planning / revise)
 * - Image + Video (render)
 */

import React, { useEffect, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { Select, Spin } from '@arco-design/web-react';
import type { IMediaModelOption } from '@/common/adapter/ipcBridge';
import { formatCloudModelLabel } from '@/renderer/utils/model/cloudModelLabel';
import { useMediaModels } from '@/renderer/hooks/agent/useMediaModels';
import { useGeneratorModels } from '@renderer/pages/workshop/generation/useGeneratorModels';

export interface VimaxModelSelection {
  llm_model: string;
  image_model: string;
  video_model: string;
}

/** Preferred default video model when present in the Flowy catalog. */
const PREFERRED_VIDEO_MODEL_NEEDLE = 'doubao-seedance-2-0-fast';

/** Preferred planning LLM catalog name (match id or display label). */
const PREFERRED_LLM_MODEL_NAME = 'Deepseek-v4-pro';

/** Image catalog is restricted to Seedream 5.0 Lite by catalog `name`. */
const ALLOWED_IMAGE_MODEL_NAME = 'Doubao-seedream-5-0-lite';

function normalizeModelKey(id: string): string {
  return id.toLowerCase().replace(/[^a-z0-9]/g, '');
}

function mediaModelLabel(model: IMediaModelOption): string {
  const name = model.name.trim();
  return name || formatCloudModelLabel(model.id);
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

interface ModelSelectorsProps {
  value: VimaxModelSelection;
  onChange: (next: VimaxModelSelection) => void;
  disabled?: boolean;
  isMobile?: boolean;
  /** Limit visible pickers for compact generation-preference surfaces. */
  mode?: 'all' | 'agent' | 'image' | 'video' | 'llm' | 'action';
  /** Mount dropdowns on body so nested popovers do not clip or auto-close. */
  popupContainer?: HTMLElement | (() => HTMLElement);
}

const ModelSelectors: React.FC<ModelSelectorsProps> = ({
  value,
  onChange,
  disabled,
  isMobile,
  mode = 'all',
  popupContainer,
}) => {
  const { t } = useTranslation();
  const llmModels = useGeneratorModels('text');
  const { imageModels, videoModels, isLoading: mediaLoading } = useMediaModels();
  const resolvePopupContainer =
    typeof popupContainer === 'function'
      ? popupContainer
      : popupContainer
        ? () => popupContainer
        : undefined;
  const selectPopupProps = resolvePopupContainer
    ? {
        getPopupContainer: resolvePopupContainer,
        triggerProps: { autoAlignPopupWidth: true as const },
      }
    : {};

  const llmOptions = useMemo(() => {
    const seen = new Set<string>();
    const opts: { label: string; value: string }[] = [];
    for (const m of llmModels.flat) {
      if (seen.has(m.model)) continue;
      seen.add(m.model);
      opts.push({
        value: m.model,
        label: `${formatCloudModelLabel(m.model)} · ${m.providerName}`,
      });
    }
    return opts;
  }, [llmModels.flat]);

  const imageOptions = useMemo(() => {
    return filterAllowedImageModels(imageModels).map((m) => ({
      value: m.id,
      label: mediaModelLabel(m),
    }));
  }, [imageModels]);

  const videoOptions = useMemo(() => {
    return videoModels.map((m) => ({
      value: m.id,
      label: mediaModelLabel(m),
    }));
  }, [videoModels]);

  // Prefer first available model when session has none yet.
  useEffect(() => {
    const patch: Partial<VimaxModelSelection> = {};
    if (mode !== 'action' && mode !== 'video' && mode !== 'image') {
      if (!value.llm_model && llmOptions[0]) {
        patch.llm_model =
          pickDefaultLlmModel(llmOptions.map((o) => o.value)) ?? llmOptions[0].value;
      }
    }
    if (mode !== 'action' && mode !== 'video') {
      if (!value.image_model && imageOptions[0]) patch.image_model = imageOptions[0].value;
      if (
        value.image_model &&
        imageOptions.length > 0 &&
        !imageOptions.some((o) => o.value === value.image_model)
      ) {
        patch.image_model = imageOptions[0].value;
      }
    }
    if (!value.video_model) {
      const preferred = pickDefaultVideoModel(videoModels);
      if (preferred) patch.video_model = preferred;
    } else if (
      videoOptions.length > 0 &&
      !videoOptions.some((o) => o.value === value.video_model)
    ) {
      const preferred = pickDefaultVideoModel(videoModels);
      if (preferred) patch.video_model = preferred;
    }
    if (Object.keys(patch).length > 0) {
      onChange({ ...value, ...patch });
    }
    // Only seed once catalogs load / when empty.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [llmOptions, imageOptions, videoModels, videoOptions, mode]);

  const showLlm = mode === 'all' || mode === 'agent' || mode === 'llm';
  const showImage = mode === 'all' || mode === 'agent' || mode === 'image';
  const showVideo =
    mode === 'all' || mode === 'agent' || mode === 'video' || mode === 'action';
  const visibleCount = [showLlm, showImage, showVideo].filter(Boolean).length;
  const grid =
    isMobile || visibleCount === 1
      ? 'grid-cols-1'
      : visibleCount === 2
        ? 'grid-cols-2'
        : 'grid-cols-3';

  return (
    <div className={`grid gap-10px ${grid}`}>
      {showLlm ? (
        <div className='flex flex-col gap-6px'>
          <label className='text-12px text-[var(--color-text-3)]'>
            {t('videoGeneration.workspace.models.llm', {
              defaultValue: '规划模型（LLM）',
            })}
          </label>
          <Select
            showSearch
            allowClear
            disabled={disabled}
            placeholder={t('videoGeneration.workspace.models.llmPlaceholder', {
              defaultValue: '选择聊天模型',
            })}
            value={value.llm_model || undefined}
            onChange={(v) => onChange({ ...value, llm_model: (v as string) || '' })}
            options={llmOptions}
            notFoundContent={
              llmModels.hasProviders
                ? t('videoGeneration.workspace.models.empty', {
                    defaultValue: '暂无可用模型',
                  })
                : t('videoGeneration.workspace.models.noProviders', {
                    defaultValue: '请先在模型中心配置平台',
                  })
            }
            {...selectPopupProps}
          />
        </div>
      ) : null}
      {showImage ? (
        <div className='flex flex-col gap-6px'>
          <label className='text-12px text-[var(--color-text-3)]'>
            {t('videoGeneration.workspace.models.image', {
              defaultValue: '图片模型',
            })}
          </label>
          {mediaLoading ? (
            <Spin size={16} />
          ) : (
            <Select
              showSearch
              allowClear
              disabled={disabled}
              placeholder={t('videoGeneration.workspace.models.imagePlaceholder', {
                defaultValue: '选择图片模型',
              })}
              value={value.image_model || undefined}
              onChange={(v) =>
                onChange({ ...value, image_model: (v as string) || '' })
              }
              options={imageOptions}
              notFoundContent={t('videoGeneration.workspace.models.empty', {
                defaultValue: '暂无可用模型',
              })}
              {...selectPopupProps}
            />
          )}
        </div>
      ) : null}
      {showVideo ? (
        <div className='flex flex-col gap-6px'>
          <label className='text-12px text-[var(--color-text-3)]'>
            {t('videoGeneration.workspace.models.video', {
              defaultValue: '视频模型',
            })}
          </label>
          {mediaLoading ? (
            <Spin size={16} />
          ) : (
            <Select
              showSearch
              allowClear
              disabled={disabled}
              placeholder={t('videoGeneration.workspace.models.videoPlaceholder', {
                defaultValue: '选择视频模型',
              })}
              value={value.video_model || undefined}
              onChange={(v) =>
                onChange({ ...value, video_model: (v as string) || '' })
              }
              options={videoOptions}
              notFoundContent={t('videoGeneration.workspace.models.empty', {
                defaultValue: '暂无可用模型',
              })}
              {...selectPopupProps}
            />
          )}
        </div>
      ) : null}
    </div>
  );
};

export default ModelSelectors;
