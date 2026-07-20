/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * Per-session Flowy model pickers for video generation:
 * - LLM (planning / revise)
 * - Image + Video (render)
 */

import React, { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Select, Spin } from '@arco-design/web-react';
import { ipcBridge } from '@/common';
import { formatCloudModelLabel } from '@/renderer/utils/model/cloudModelLabel';
import { useGeneratorModels } from '@renderer/pages/workshop/generation/useGeneratorModels';

export interface VimaxModelSelection {
  llm_model: string;
  image_model: string;
  video_model: string;
}

interface ModelSelectorsProps {
  value: VimaxModelSelection;
  onChange: (next: VimaxModelSelection) => void;
  disabled?: boolean;
  isMobile?: boolean;
}

const ModelSelectors: React.FC<ModelSelectorsProps> = ({
  value,
  onChange,
  disabled,
  isMobile,
}) => {
  const { t } = useTranslation();
  const llmModels = useGeneratorModels('text');
  const [mediaLoading, setMediaLoading] = useState(true);
  const [imageModels, setImageModels] = useState<string[]>([]);
  const [videoModels, setVideoModels] = useState<string[]>([]);

  useEffect(() => {
    let cancelled = false;
    setMediaLoading(true);
    ipcBridge.media.listModels
      .invoke()
      .then((list) => {
        if (cancelled) return;
        setImageModels(list?.image_models ?? []);
        setVideoModels(list?.video_models ?? []);
      })
      .catch((e) => {
        console.warn('[videoGeneration] list media models failed', e);
        if (!cancelled) {
          setImageModels([]);
          setVideoModels([]);
        }
      })
      .finally(() => {
        if (!cancelled) setMediaLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

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

  // Prefer first available model when session has none yet.
  useEffect(() => {
    const patch: Partial<VimaxModelSelection> = {};
    if (!value.llm_model && llmOptions[0]) patch.llm_model = llmOptions[0].value;
    if (!value.image_model && imageModels[0]) patch.image_model = imageModels[0];
    if (!value.video_model && videoModels[0]) patch.video_model = videoModels[0];
    if (Object.keys(patch).length > 0) {
      onChange({ ...value, ...patch });
    }
    // Only seed once catalogs load / when empty.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [llmOptions, imageModels, videoModels]);

  const grid = isMobile ? 'grid-cols-1' : 'grid-cols-3';

  return (
    <div className={`grid gap-10px ${grid}`}>
      <div className='flex flex-col gap-6px'>
        <label className='text-12px text-[var(--color-text-3)]'>
          {t('videoGeneration.workspace.models.llm', { defaultValue: '规划模型（LLM）' })}
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
              ? t('videoGeneration.workspace.models.empty', { defaultValue: '暂无可用模型' })
              : t('videoGeneration.workspace.models.noProviders', {
                  defaultValue: '请先在模型中心配置平台',
                })
          }
        />
      </div>
      <div className='flex flex-col gap-6px'>
        <label className='text-12px text-[var(--color-text-3)]'>
          {t('videoGeneration.workspace.models.image', { defaultValue: '图片模型' })}
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
            onChange={(v) => onChange({ ...value, image_model: (v as string) || '' })}
            options={imageModels.map((id) => ({
              value: id,
              label: formatCloudModelLabel(id),
            }))}
            notFoundContent={t('videoGeneration.workspace.models.empty', {
              defaultValue: '暂无可用模型',
            })}
          />
        )}
      </div>
      <div className='flex flex-col gap-6px'>
        <label className='text-12px text-[var(--color-text-3)]'>
          {t('videoGeneration.workspace.models.video', { defaultValue: '视频模型' })}
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
            onChange={(v) => onChange({ ...value, video_model: (v as string) || '' })}
            options={videoModels.map((id) => ({
              value: id,
              label: formatCloudModelLabel(id),
            }))}
            notFoundContent={t('videoGeneration.workspace.models.empty', {
              defaultValue: '暂无可用模型',
            })}
          />
        )}
      </div>
    </div>
  );
};

export default ModelSelectors;
