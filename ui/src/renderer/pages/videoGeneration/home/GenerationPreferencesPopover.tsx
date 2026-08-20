import React, { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { Button, ConfigProvider, Select, Switch } from '@arco-design/web-react';
import { Down, SettingTwo } from '@icon-park/react';
import { useTranslation } from 'react-i18next';
import { formatCloudModelLabel } from '@/renderer/utils/model/cloudModelLabel';
import { useMediaModels } from '@/renderer/hooks/agent/useMediaModels';
import { useGeneratorModels } from '@renderer/pages/workshop/generation/useGeneratorModels';
import { SEEDANCE_ASPECT_RATIOS, type SeedanceAspectRatio } from '../aspectRatios';
import DurationTimelineBar from '../components/DurationTimelineBar';
import {
  AGENT_TICKS,
  CLIP_DURATION_MAX_SECS,
  CLIP_DURATION_MIN_SECS,
  CLIP_DURATION_STEP_SECS,
  CLIP_TICKS,
  clampDuration,
  DURATION_MAX_SECS,
  DURATION_MIN_SECS,
  DURATION_STEP_SECS,
} from '../durationBounds';
import {
  filterAllowedImageModels,
  pickDefaultLlmModel,
  pickDefaultVideoModel,
} from '../components/ModelSelectors';
import {
  normalizeVideoFps,
  normalizeVideoResolution,
  videoModelCapabilities,
} from '../videoModelCapabilities';
import type { GenerationPreferences, VideoHomeMode } from './types';
import type { VimaxWorkflow } from '../types';
import { getScrollParents } from './scrollParents';
import styles from './home.module.css';

const PREFS_RATIO_ORDER: readonly SeedanceAspectRatio[] = [
  '21:9',
  '16:9',
  '4:3',
  '1:1',
  '3:4',
  '9:16',
];

const PANEL_WIDTH = 448;
const PANEL_MARGIN = 12;
const PANEL_GAP = 8;

interface GenerationPreferencesPopoverProps {
  mode: VideoHomeMode;
  value: GenerationPreferences;
  disabled?: boolean;
  modelMissing?: boolean;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onChange: (next: GenerationPreferences) => void;
  onOpenModelHub: () => void;
  workflow?: VimaxWorkflow;
}

type PanelPlacement = {
  top?: number;
  bottom?: number;
  left: number;
  width: number;
  maxHeight: number;
};

function AspectRatioIcon({
  ratio,
  smart,
}: {
  ratio?: SeedanceAspectRatio;
  smart?: boolean;
}) {
  if (smart) {
    return <span className={styles.ratioIconSmart} aria-hidden='true' />;
  }
  const [width, height] = (ratio ?? '1:1').split(':').map(Number);
  const landscape = width >= height;
  return (
    <span
      className={styles.ratioIcon}
      style={{
        width: landscape ? 20 : Math.max(9, (20 * width) / height),
        height: landscape ? Math.max(9, (20 * height) / width) : 20,
      }}
      aria-hidden='true'
    />
  );
}

function shortModelLabel(modelId: string, emptyLabel: string): string {
  const raw = modelId.trim();
  if (!raw) return emptyLabel;
  const tail = raw.split(/[/:@]/).pop() || raw;
  return tail.length > 14 ? `${tail.slice(0, 12)}…` : tail;
}

function isInsideSelectUi(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return false;
  return Boolean(
    target.closest(
      [
        '.arco-select',
        '.arco-select-view',
        '.arco-select-popup',
        '.arco-select-dropdown',
        '.arco-select-option',
        '.arco-cascader-popup',
        '.arco-trigger',
        '.arco-trigger-popup',
        '.video-home-prefs-select-popup',
        '[class*="arco-select-popup"]',
        '[class*="arco-trigger-popup"]',
      ].join(',')
    )
  );
}

function isVideoHomeSubmitTarget(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return false;
  return Boolean(target.closest('[data-video-home-submit]'));
}

const SELECT_POPUP_Z_INDEX = 1600;
/** Stable global class — Arco Trigger overwrites popupStyle.zIndex from context. */
const SELECT_POPUP_CLASS = 'video-home-prefs-select-popup';

function isSelectPopupOpen(): boolean {
  return Boolean(
    document.querySelector(
      [
        '.video-home-prefs-select-popup',
        '.arco-select-popup:not(.arco-select-popup-hidden)',
        '.arco-trigger-popup:not(.arco-trigger-popup-hidden)',
      ].join(', ')
    )
  );
}

function durationBounds(mode: VideoHomeMode) {
  if (mode === 'creation') {
    return {
      min: CLIP_DURATION_MIN_SECS,
      max: CLIP_DURATION_MAX_SECS,
      step: CLIP_DURATION_STEP_SECS,
      ticks: CLIP_TICKS,
    };
  }
  return {
    min: DURATION_MIN_SECS,
    max: DURATION_MAX_SECS,
    step: DURATION_STEP_SECS,
    ticks: AGENT_TICKS,
  };
}

const GenerationPreferencesPopover: React.FC<GenerationPreferencesPopoverProps> = ({
  mode,
  value,
  disabled,
  modelMissing,
  open,
  onOpenChange,
  onChange,
  onOpenModelHub,
  workflow,
}) => {
  const { t } = useTranslation();
  const isAction = mode === 'action' || workflow === 'action2video';
  const anchorRef = useRef<HTMLDivElement | null>(null);
  const panelRef = useRef<HTMLDivElement | null>(null);
  const panelPosRef = useRef<PanelPlacement>({
    left: 0,
    width: PANEL_WIDTH,
    maxHeight: 640,
  });
  const [panelPos, setPanelPos] = useState<PanelPlacement>(panelPosRef.current);
  const valueRef = useRef(value);
  valueRef.current = value;

  // Defer catalog network until the panel opens — closed summary only needs
  // already-persisted preference ids / labels.
  const llmModels = useGeneratorModels('text', { enabled: open && !isAction });
  const { imageModels, videoModels, isLoading: mediaLoading } = useMediaModels({
    enabled: open,
  });
  const mediaKind = isAction ? 'video' : value.mediaKind;
  const duration = durationBounds(mode);

  const automaticLabel = t('videoGeneration.create.preferences.automatic', {
    defaultValue: '自动',
  });
  const smartLabel = t('videoGeneration.create.preferences.smart', {
    defaultValue: '智能',
  });
  const imageLabel = t('videoGeneration.create.preferences.image', {
    defaultValue: '图片',
  });
  const videoLabel = t('videoGeneration.create.preferences.video', {
    defaultValue: '视频',
  });
  const noModelLabel = t('videoGeneration.create.preferences.noModelSelected', {
    defaultValue: '未选模型',
  });
  const loadingLabel = t('videoGeneration.create.preferences.loading', {
    defaultValue: '加载中…',
  });
  const emptyModelsLabel = t('videoGeneration.workspace.models.empty', {
    defaultValue: '暂无可用模型',
  });

  const summary = isAction
    ? `${shortModelLabel(value.models.video_model, noModelLabel)} · ${value.resolution.toUpperCase()}`
    : value.automatic
      ? automaticLabel
      : mediaKind === 'image'
        ? `${value.smartAspect ? smartLabel : value.aspectRatio} · ${shortModelLabel(
            value.models.image_model,
            noModelLabel
          )}`
        : `${value.smartAspect ? smartLabel : value.aspectRatio} · ${value.resolution.toUpperCase()}`;

  const summaryTitle = isAction
    ? summary
    : value.automatic
      ? automaticLabel
      : `${mediaKind === 'image' ? imageLabel : videoLabel} · ${
          value.smartAspect ? smartLabel : value.aspectRatio
        } · ${
          mediaKind === 'video'
            ? value.resolution.toUpperCase()
            : shortModelLabel(value.models.image_model, noModelLabel)
        }`;

  const llmOptions = useMemo(() => {
    const seen = new Set<string>();
    const opts: { label: string; value: string }[] = [];
    for (const model of llmModels.flat) {
      if (seen.has(model.model)) continue;
      seen.add(model.model);
      opts.push({
        value: model.model,
        label: `${formatCloudModelLabel(model.model)} · ${model.providerName}`,
      });
    }
    return opts;
  }, [llmModels.flat]);

  const imageOptions = useMemo(
    () =>
      filterAllowedImageModels(imageModels).map((model) => ({
        value: model.id,
        label: model.name.trim() || formatCloudModelLabel(model.id),
      })),
    [imageModels]
  );

  const videoOptions = useMemo(
    () =>
      videoModels.map((model) => ({
        value: model.id,
        label: model.name.trim() || formatCloudModelLabel(model.id),
      })),
    [videoModels]
  );

  const resolutionOptions = useMemo(
    () => videoModelCapabilities(value.models.video_model).resolutions,
    [value.models.video_model]
  );

  const orderedRatios = useMemo(() => {
    const known = new Set<string>(SEEDANCE_ASPECT_RATIOS);
    return PREFS_RATIO_ORDER.filter((ratio) => known.has(ratio));
  }, []);

  const safeImageValue = imageOptions.some(
    (option) => option.value === value.models.image_model
  )
    ? value.models.image_model
    : undefined;
  const safeVideoValue = videoOptions.some(
    (option) => option.value === value.models.video_model
  )
    ? value.models.video_model
    : undefined;
  const safeResolution = normalizeVideoResolution(
    value.models.video_model,
    value.resolution
  );
  const safeLlmValue = llmOptions.some(
    (option) => option.value === value.models.llm_model
  )
    ? value.models.llm_model
    : undefined;

  const updatePanelPosition = () => {
    const anchor = anchorRef.current;
    if (!anchor) return;
    const rect = anchor.getBoundingClientRect();
    const width = Math.min(PANEL_WIDTH, window.innerWidth - PANEL_MARGIN * 2);
    const left = Math.min(
      Math.max(PANEL_MARGIN, rect.left),
      Math.max(PANEL_MARGIN, window.innerWidth - width - PANEL_MARGIN)
    );

    const measuredHeight = panelRef.current?.offsetHeight;
    const estimatedHeight = measuredHeight && measuredHeight > 0
      ? measuredHeight
      : Math.min(window.innerHeight * 0.72, 560);
    const topSpace = rect.top - PANEL_GAP - PANEL_MARGIN;
    const bottomSpace = window.innerHeight - rect.bottom - PANEL_GAP - PANEL_MARGIN;
    const placeAbove =
      bottomSpace < Math.min(estimatedHeight, 360) && topSpace > bottomSpace;

    const next: PanelPlacement = placeAbove
      ? {
          left,
          width,
          bottom: window.innerHeight - rect.top + PANEL_GAP,
          maxHeight: Math.max(240, topSpace),
        }
      : {
          left,
          width,
          top: rect.bottom + PANEL_GAP,
          maxHeight: Math.max(240, bottomSpace),
        };

    const prev = panelPosRef.current;
    if (
      prev.left === next.left &&
      prev.width === next.width &&
      prev.maxHeight === next.maxHeight &&
      prev.top === next.top &&
      prev.bottom === next.bottom
    ) {
      return;
    }
    panelPosRef.current = next;
    setPanelPos(next);
  };

  useLayoutEffect(() => {
    if (!open) return;
    updatePanelPosition();
    const frame = window.requestAnimationFrame(() => updatePanelPosition());
    return () => window.cancelAnimationFrame(frame);
  }, [open, mediaKind, value.automatic, mode, modelMissing]);

  useEffect(() => {
    if (!open) return;
    const onReposition = () => updatePanelPosition();
    // Do NOT use capture scroll — panel internal scrolling would re-render and break Select.
    // Page scroll lives on an overflow container (not window); listen to those parents too.
    window.addEventListener('resize', onReposition);
    window.addEventListener('scroll', onReposition);
    const scrollParents = getScrollParents(anchorRef.current);
    scrollParents.forEach((el) =>
      el.addEventListener('scroll', onReposition, { passive: true })
    );
    return () => {
      window.removeEventListener('resize', onReposition);
      window.removeEventListener('scroll', onReposition);
      scrollParents.forEach((el) => el.removeEventListener('scroll', onReposition));
    };
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target;
      if (!(target instanceof Node)) return;
      if (anchorRef.current?.contains(target)) return;
      if (panelRef.current?.contains(target)) return;
      if (isVideoHomeSubmitTarget(target) || isInsideSelectUi(target) || isSelectPopupOpen()) {
        return;
      }
      onOpenChange(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      // Let an open Select consume Escape first; don't tear down the panel.
      if (event.key === 'Escape' && !isSelectPopupOpen()) onOpenChange(false);
    };
    // Bubble phase so Select can open first; ignore select UI targets.
    document.addEventListener('pointerdown', onPointerDown);
    document.addEventListener('keydown', onKeyDown);
    return () => {
      document.removeEventListener('pointerdown', onPointerDown);
      document.removeEventListener('keydown', onKeyDown);
    };
  }, [open, onOpenChange]);

  // Clamp resolution / fps whenever the video model allow-list changes.
  useEffect(() => {
    const current = valueRef.current;
    if (!current.models.video_model) return;
    const resolution = normalizeVideoResolution(
      current.models.video_model,
      current.resolution
    );
    const fps = normalizeVideoFps(current.models.video_model, current.fps);
    if (resolution === current.resolution && fps === current.fps) return;
    onChange({ ...current, resolution, fps });
    // eslint-disable-next-line react-hooks/exhaustive-deps -- only re-clamp on model / allow-list
  }, [value.models.video_model, resolutionOptions.join(',')]);

  // Seed missing / invalid models whenever options become available while open.
  useEffect(() => {
    if (!open) return;
    if (mediaLoading && imageOptions.length === 0 && videoOptions.length === 0) return;

    const current = valueRef.current;
    const patch: Partial<GenerationPreferences['models']> = {};
    if (!isAction && !current.models.llm_model && llmOptions[0]) {
      patch.llm_model =
        pickDefaultLlmModel(llmOptions.map((option) => option.value)) ?? llmOptions[0].value;
    } else if (
      !isAction &&
      current.models.llm_model &&
      llmOptions.length > 0 &&
      !llmOptions.some((option) => option.value === current.models.llm_model)
    ) {
      patch.llm_model =
        pickDefaultLlmModel(llmOptions.map((option) => option.value)) ?? llmOptions[0].value;
    }
    if (!isAction && !current.models.image_model && imageOptions[0]) {
      patch.image_model = imageOptions[0].value;
    } else if (
      !isAction &&
      current.models.image_model &&
      imageOptions.length > 0 &&
      !imageOptions.some((option) => option.value === current.models.image_model)
    ) {
      patch.image_model = imageOptions[0].value;
    }
    const preferredVideo = pickDefaultVideoModel(videoModels);
    if (!current.models.video_model) {
      if (preferredVideo) patch.video_model = preferredVideo;
    } else if (
      videoOptions.length > 0 &&
      !videoOptions.some((option) => option.value === current.models.video_model)
    ) {
      if (preferredVideo) patch.video_model = preferredVideo;
    }
    if (Object.keys(patch).length === 0) return;
    onChange({ ...current, models: { ...current.models, ...patch } });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [
    open,
    mediaLoading,
    llmOptions,
    imageOptions,
    videoOptions,
    videoModels,
    value.models.llm_model,
    value.models.image_model,
    value.models.video_model,
    isAction,
  ]);

  const setMediaKind = (kind: GenerationPreferences['mediaKind']) => {
    if (kind === mediaKind) return;
    onChange({ ...value, mediaKind: kind });
  };

  // Arco Trigger applies context zIndex AFTER popupStyle, wiping inline z-index
  // when unset — default CSS is 1000, below this panel (1100). ConfigProvider +
  // a global popup class keep menus above the card even with a single option.
  const selectProps = {
    getPopupContainer: () => document.body,
    triggerProps: {
      updateOnScroll: true,
      autoFitPosition: true,
      className: SELECT_POPUP_CLASS,
      popupStyle: {
        minWidth: 280,
      },
    },
    dropdownMenuStyle: { maxHeight: 280 },
    dropdownMenuClassName: styles.prefsSelectMenu,
  };

  const panel = open
    ? createPortal(
        <ConfigProvider zIndex={SELECT_POPUP_Z_INDEX}>
          <div
            ref={panelRef}
            className={styles.preferencesFloating}
            style={{
              left: panelPos.left,
              width: panelPos.width,
              maxHeight: panelPos.maxHeight,
              ...(panelPos.top != null
                ? { top: panelPos.top }
                : { bottom: panelPos.bottom }),
            }}
            role='dialog'
            aria-label={t('videoGeneration.create.preferences.dialogAria', {
              defaultValue: '生成偏好',
            })}
          >
            <div className={styles.preferencesPanel}>
              <div className={styles.preferencesHeader}>
                <div className={styles.preferencesTitle}>
                  {t('videoGeneration.create.preferences.title', {
                    defaultValue: '生成偏好',
                  })}
                </div>
                {isAction ? null : (
                <label className={styles.autoToggle}>
                  <span>{automaticLabel}</span>
                  <Switch
                    size='small'
                    checked={value.automatic}
                    disabled={disabled}
                    onChange={(automatic) => onChange({ ...value, automatic })}
                  />
                </label>
                )}
              </div>

            {isAction ? null : (
            <div
              className={styles.mediaKindTabs}
              role='tablist'
              aria-label={t('videoGeneration.create.preferences.mediaKindAria', {
                defaultValue: '生成类型',
              })}
            >
              {(['image', 'video'] as const).map((kind) => (
                <button
                  key={kind}
                  type='button'
                  role='tab'
                  aria-selected={mediaKind === kind}
                  disabled={disabled}
                  className={`${styles.mediaKindTab} ${
                    mediaKind === kind ? styles.mediaKindTabActive : ''
                  }`}
                  onClick={() => setMediaKind(kind)}
                >
                  {kind === 'image' ? imageLabel : videoLabel}
                </button>
              ))}
            </div>
            )}

            {isAction || !value.automatic ? null : (
              <div className={styles.autoHint}>
                {t('videoGeneration.create.preferences.automaticHint', {
                  defaultValue:
                    '已开启自动，比例与模型选项已锁定；提交时使用当前已选配置。',
                })}
              </div>
            )}

            {isAction ? null : (
            <div
              className={`${styles.preferenceSection} ${
                value.automatic ? styles.preferenceSectionMuted : ''
              }`}
            >
              <div className={styles.preferenceLabel}>
                {t('videoGeneration.create.preferences.aspectLabel', {
                  defaultValue: '选择比例',
                })}
              </div>
              <div
                className={styles.ratioGrid}
                role='radiogroup'
                aria-label={t('videoGeneration.create.preferences.aspectAria', {
                  defaultValue: '生成比例',
                })}
              >
                <button
                  type='button'
                  role='radio'
                  aria-checked={!value.automatic && value.smartAspect}
                  disabled={disabled || value.automatic}
                  className={`${styles.ratioButton} ${
                    !value.automatic && value.smartAspect ? styles.ratioButtonActive : ''
                  }`}
                  onClick={() =>
                    onChange({ ...value, smartAspect: true, automatic: false })
                  }
                >
                  <AspectRatioIcon smart />
                  <span>{smartLabel}</span>
                </button>
                {orderedRatios.map((ratio) => (
                  <button
                    key={ratio}
                    type='button'
                    role='radio'
                    aria-checked={
                      !value.automatic &&
                      !value.smartAspect &&
                      value.aspectRatio === ratio
                    }
                    disabled={disabled || value.automatic}
                    className={`${styles.ratioButton} ${
                      !value.automatic &&
                      !value.smartAspect &&
                      value.aspectRatio === ratio
                        ? styles.ratioButtonActive
                        : ''
                    }`}
                    onClick={() =>
                      onChange({
                        ...value,
                        aspectRatio: ratio,
                        smartAspect: false,
                        automatic: false,
                      })
                    }
                  >
                    <AspectRatioIcon ratio={ratio} />
                    <span>{ratio}</span>
                  </button>
                ))}
              </div>
            </div>
            )}

            <div
              className={`${styles.preferenceSection} ${
                value.automatic && !isAction ? styles.preferenceSectionMuted : ''
              }`}
            >
              <div className={styles.preferenceLabel}>
                {mediaKind === 'image'
                  ? t('videoGeneration.create.preferences.imageModel', {
                      defaultValue: '图片模型',
                    })
                  : t('videoGeneration.create.preferences.modelAndResolution', {
                      defaultValue: '模型与清晰度',
                    })}
              </div>
              <div className={styles.modelSettingsStack}>
                {mediaKind === 'image' ? (
                  <Select
                    allowClear={false}
                    disabled={disabled || value.automatic || mediaLoading}
                    placeholder={t('videoGeneration.workspace.models.imagePlaceholder', {
                      defaultValue: '选择图片模型',
                    })}
                    value={safeImageValue}
                    options={imageOptions}
                    loading={mediaLoading}
                    notFoundContent={mediaLoading ? loadingLabel : emptyModelsLabel}
                    onChange={(next) =>
                      onChange({
                        ...value,
                        mediaKind: 'image',
                        models: {
                          ...value.models,
                          image_model: String(next ?? ''),
                        },
                        automatic: false,
                      })
                    }
                    {...selectProps}
                  />
                ) : (
                  <>
                    <Select
                      allowClear={false}
                      disabled={disabled || (!isAction && value.automatic) || mediaLoading}
                      placeholder={t('videoGeneration.workspace.models.videoPlaceholder', {
                        defaultValue: '选择视频模型',
                      })}
                      value={safeVideoValue}
                      options={videoOptions}
                      loading={mediaLoading}
                      notFoundContent={
                        mediaLoading
                          ? loadingLabel
                          : isAction
                            ? t('videoGeneration.workspace.models.h3Empty', {
                                defaultValue: '暂无 MiniMax-H3 视频模型',
                              })
                            : emptyModelsLabel
                      }
                      onChange={(next) => {
                        const video_model = String(next ?? '');
                        onChange({
                          ...value,
                          mediaKind: 'video',
                          models: {
                            ...value.models,
                            video_model,
                          },
                          resolution: normalizeVideoResolution(
                            video_model,
                            value.resolution
                          ),
                          fps: normalizeVideoFps(video_model, value.fps),
                          automatic: false,
                        });
                      }}
                      {...selectProps}
                    />
                    <div
                      className={styles.resolutionPills}
                      role='radiogroup'
                      aria-label={t('videoGeneration.create.preferences.resolutionAria', {
                        defaultValue: '清晰度',
                      })}
                    >
                      {resolutionOptions.map((resolution) => {
                        const active = resolution === safeResolution;
                        return (
                          <button
                            key={resolution}
                            type='button'
                            role='radio'
                            aria-checked={active}
                            disabled={disabled || (!isAction && value.automatic)}
                            className={`${styles.resolutionPill} ${
                              active ? styles.resolutionPillActive : ''
                            }`}
                            onClick={() => {
                              if (disabled || (!isAction && value.automatic) || active) return;
                              onChange({
                                ...value,
                                mediaKind: 'video',
                                resolution,
                                automatic: false,
                              });
                            }}
                          >
                            {resolution.toUpperCase()}
                          </button>
                        );
                      })}
                    </div>
                  </>
                )}
              </div>
            </div>

            {mediaKind === 'video' && mode === 'creation' ? (
              <div
                className={`${styles.preferenceSection} ${
                  value.automatic ? styles.preferenceSectionMuted : ''
                }`}
              >
                <div className={styles.preferenceLabel}>
                  {mode === 'creation'
                    ? t('videoGeneration.create.preferences.clipDuration', {
                        defaultValue: '单段时长',
                      })
                    : t('videoGeneration.create.preferences.targetDuration', {
                        defaultValue: '目标时长',
                      })}
                </div>
                <div className={styles.durationWrap}>
                  <DurationTimelineBar
                    value={clampDuration(
                      value.targetDurationSecs,
                      duration.min,
                      duration.max,
                      duration.step
                    )}
                    disabled={disabled || value.automatic}
                    hideLabel
                    min={duration.min}
                    max={duration.max}
                    step={duration.step}
                    ticks={duration.ticks}
                    onChange={(targetDurationSecs) =>
                      onChange({
                        ...value,
                        mediaKind: 'video',
                        targetDurationSecs: clampDuration(
                          targetDurationSecs,
                          duration.min,
                          duration.max,
                          duration.step
                        ),
                        automatic: false,
                      })
                    }
                  />
                </div>
              </div>
            ) : null}

            {mode === 'agent' && !isAction ? (
              <div
                className={`${styles.preferenceSection} ${
                  value.automatic ? styles.preferenceSectionMuted : ''
                }`}
              >
                <div className={styles.preferenceLabel}>
                  {t('videoGeneration.create.preferences.planningModel', {
                    defaultValue: '规划模型',
                  })}
                </div>
                <Select
                  allowClear={false}
                  disabled={disabled || value.automatic}
                  placeholder={t('videoGeneration.workspace.models.llmPlaceholder', {
                    defaultValue: '选择聊天模型',
                  })}
                  value={safeLlmValue}
                  options={llmOptions}
                  notFoundContent={emptyModelsLabel}
                  onChange={(next) =>
                    onChange({
                      ...value,
                      models: { ...value.models, llm_model: String(next ?? '') },
                    })
                  }
                  {...selectProps}
                  triggerProps={{
                    ...selectProps.triggerProps,
                    // Near the bottom of the card: open upward so the menu
                    // sits over the panel instead of under/outside it.
                    position: 'tl',
                  }}
                />
              </div>
            ) : null}

            {modelMissing ? (
              <div className={styles.modelWarning}>
                <span>
                  {t(
                    isAction
                      ? 'videoGeneration.create.modelRequiredAction'
                      : 'videoGeneration.create.modelRequired',
                    {
                      defaultValue: isAction
                        ? '生成前需要选择视频模型。'
                        : '生成前需要可用的规划模型。',
                    }
                  )}
                </span>
                <Button type='text' size='mini' onClick={onOpenModelHub}>
                  {t('videoGeneration.create.configureModels', {
                    defaultValue: '前往模型中心',
                  })}
                </Button>
              </div>
            ) : null}
            </div>
          </div>
        </ConfigProvider>,
        document.body
      )
    : null;

  return (
    <div className={styles.prefsAnchor} ref={anchorRef}>
      <button
        type='button'
        className={`${styles.toolbarButton} ${styles.prefsButton} ${
          open ? styles.toolbarButtonActive : ''
        }`}
        disabled={disabled}
        aria-expanded={open}
        aria-haspopup='dialog'
        onClick={() => onOpenChange(!open)}
        aria-label={t('videoGeneration.create.customize', {
          defaultValue: '自定义生成偏好',
        })}
      >
        <SettingTwo theme='outline' size={15} />
        <span className={styles.toolbarLabel}>
          {t('videoGeneration.create.preferences.customize', {
            defaultValue: '自定义',
          })}
        </span>
        <span className={styles.toolbarSummary} title={summaryTitle}>
          {summary}
        </span>
        <Down theme='outline' size={12} />
      </button>
      {panel}
    </div>
  );
};

export default GenerationPreferencesPopover;
