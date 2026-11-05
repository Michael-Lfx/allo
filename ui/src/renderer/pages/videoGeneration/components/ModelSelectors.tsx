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
import { useEffectiveModelTier } from '@renderer/hooks/cloud/useEffectiveModelTier';
import { isPlanLocked } from '@/renderer/utils/model/planGate';
import { useMediaModels } from '@/renderer/hooks/agent/useMediaModels';
import { useGeneratorModels } from '@renderer/pages/workshop/generation/useGeneratorModels';
import {
  VideoModelOptionLabel,
  videoModelSelectClassName,
  videoModelSelectFilter,
} from '@/renderer/components/model/VideoModelOptionLabel';
import {
  filterAllowedImageModels,
  pickDefaultLlmModel,
  pickDefaultVideoModel,
} from './modelPreferenceDefaults';

export interface VimaxModelSelection {
  llm_model: string;
  image_model: string;
  video_model: string;
}

export { filterAllowedImageModels, pickDefaultLlmModel, pickDefaultVideoModel };

function mediaModelLabel(model: IMediaModelOption): string {
  const name = model.name.trim();
  return name || formatCloudModelLabel(model.id);
}

function planBadgeName(name: string | undefined, fallback: string): string {
  const trimmed = name?.trim();
  return trimmed || fallback;
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
  const profileTier = useEffectiveModelTier();
  const llmModels = useGeneratorModels('text');
  const { imageModels, videoModels, effectiveModelTier: mediaTier, isLoading: mediaLoading } = useMediaModels();
  const effectiveTier = Math.max(profileTier, mediaTier ?? 0);
  const memberLabel = t('conversation.modelPicker.member', { defaultValue: 'Member' });
  const planBadge = (name: string | undefined) => (
    <span className='chat-model-plan-badge shrink-0'>{planBadgeName(name, memberLabel)}</span>
  );
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
    const opts: { label: React.ReactNode; value: string; disabled?: boolean; locked?: boolean }[] = [];
    for (const m of llmModels.flat) {
      if (seen.has(m.model)) continue;
      seen.add(m.model);
      const locked = isPlanLocked(m.minTierLevel ?? 0, effectiveTier);
      opts.push({
        value: m.model,
        disabled: locked,
        locked,
        label: (
          <span className='flex min-w-0 items-center gap-6px'>
            <span className='min-w-0 truncate'>{`${formatCloudModelLabel(m.model)} · ${m.providerName}`}</span>
            {locked ? planBadge(m.requiredPlanName) : null}
          </span>
        ),
      });
    }
    return [...opts.filter((option) => !option.locked), ...opts.filter((option) => option.locked)];
  }, [effectiveTier, llmModels.flat, memberLabel]);

  const imageOptions = useMemo(() => {
    const opts = filterAllowedImageModels(imageModels).map((m) => {
      const locked = isPlanLocked(m.min_tier_level ?? 0, effectiveTier);
      return {
        value: m.id,
        disabled: locked,
        locked,
        label: (
          <span className='flex min-w-0 items-center gap-6px'>
            <span className='min-w-0 truncate'>{mediaModelLabel(m)}</span>
            {locked ? planBadge(m.required_plan_name) : null}
          </span>
        ),
      };
    });
    return [...opts.filter((option) => !option.locked), ...opts.filter((option) => option.locked)];
  }, [effectiveTier, imageModels, memberLabel]);

  const videoOptions = useMemo(() => {
    const opts = videoModels.map((m) => {
      const locked = isPlanLocked(m.min_tier_level ?? 0, effectiveTier);
      return {
        value: m.id,
        label: mediaModelLabel(m),
        locked,
        planName: m.required_plan_name,
      };
    });
    return [...opts.filter((option) => !option.locked), ...opts.filter((option) => option.locked)];
  }, [effectiveTier, videoModels]);

  const videoSelectOptions = useMemo(
    () =>
      videoOptions.map((option) => ({
        value: option.value,
        disabled: option.locked,
        extra: option.label,
        label: (
          <span className='flex min-w-0 items-center gap-6px'>
            <VideoModelOptionLabel name={option.label} model={option.value} />
            {option.locked ? (
              <span className='chat-model-plan-badge shrink-0'>{planBadgeName(option.planName, memberLabel)}</span>
            ) : null}
          </span>
        ),
      })),
    [memberLabel, videoOptions]
  );

  // Prefer first available model when session has none yet.
  useEffect(() => {
    const patch: Partial<VimaxModelSelection> = {};
    const openLlm = llmOptions.filter((option) => !option.locked);
    const openImage = imageOptions.filter((option) => !option.locked);
    const openVideo = videoModels.filter((model) => !isPlanLocked(model.min_tier_level ?? 0, effectiveTier));
    if (mode !== 'action' && mode !== 'video' && mode !== 'image') {
      if (!value.llm_model && openLlm[0]) {
        patch.llm_model = pickDefaultLlmModel(openLlm.map((o) => o.value)) ?? openLlm[0].value;
      }
    }
    if (mode !== 'action' && mode !== 'video') {
      if (!value.image_model && openImage[0]) patch.image_model = openImage[0].value;
      if (
        value.image_model &&
        imageOptions.length > 0 &&
        !imageOptions.some((o) => o.value === value.image_model)
      ) {
        if (openImage[0]) patch.image_model = openImage[0].value;
      }
    }
    if (!value.video_model) {
      const preferred = pickDefaultVideoModel(openVideo);
      if (preferred) patch.video_model = preferred;
    } else if (
      videoOptions.length > 0 &&
      !videoOptions.some((o) => o.value === value.video_model)
    ) {
      const preferred = pickDefaultVideoModel(openVideo);
      if (preferred) patch.video_model = preferred;
    }
    if (Object.keys(patch).length > 0) {
      onChange({ ...value, ...patch });
    }
    // Only seed once catalogs load / when empty.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [effectiveTier, llmOptions, imageOptions, videoModels, videoOptions, mode]);

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
              className={videoModelSelectClassName}
              showSearch
              filterOption={videoModelSelectFilter}
              allowClear
              disabled={disabled}
              placeholder={t('videoGeneration.workspace.models.videoPlaceholder', {
                defaultValue: '选择视频模型',
              })}
              value={value.video_model || undefined}
              onChange={(v) =>
                onChange({ ...value, video_model: (v as string) || '' })
              }
              options={videoSelectOptions}
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
