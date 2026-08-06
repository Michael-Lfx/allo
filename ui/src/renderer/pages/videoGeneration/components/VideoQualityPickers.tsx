/**
 * Per-model resolution + fps pills for Style & Model.
 * Seedance fast/mini hide 1080p; fps is usually locked at 24.
 */
import React, { useEffect, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import {
  DEFAULT_VIDEO_FPS,
  DEFAULT_VIDEO_RESOLUTION,
  normalizeVideoFps,
  normalizeVideoResolution,
  videoModelCapabilities,
  type VideoResolution,
} from '../videoModelCapabilities';

export interface VideoQualityValue {
  resolution: VideoResolution;
  fps: number;
}

export interface VideoQualityPickersProps {
  videoModel: string;
  value: VideoQualityValue;
  onChange: (next: VideoQualityValue) => void;
  disabled?: boolean;
  className?: string;
}

const VideoQualityPickers: React.FC<VideoQualityPickersProps> = ({
  videoModel,
  value,
  onChange,
  disabled,
  className,
}) => {
  const { t } = useTranslation();
  const caps = useMemo(() => videoModelCapabilities(videoModel), [videoModel]);

  // Clamp when the video model (or its allow-list) changes.
  useEffect(() => {
    const resolution = normalizeVideoResolution(videoModel, value.resolution);
    const fps = normalizeVideoFps(videoModel, value.fps);
    if (resolution !== value.resolution || fps !== value.fps) {
      onChange({ resolution, fps });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps -- only re-clamp on model / allow-list
  }, [videoModel, caps.resolutions.join(','), caps.fpsOptions.join(',')]);

  const currentRes = normalizeVideoResolution(videoModel, value.resolution || DEFAULT_VIDEO_RESOLUTION);
  const currentFps = normalizeVideoFps(videoModel, value.fps || DEFAULT_VIDEO_FPS);

  return (
    <div className={`grid grid-cols-1 gap-10px md:grid-cols-2 ${className ?? ''}`.trim()}>
      <div className='flex flex-col gap-6px text-12px text-[var(--color-text-3)]'>
        <span>
          {t('videoGeneration.workspace.source.resolutionLabel', {
            defaultValue: '清晰度',
          })}
        </span>
        <div className='flex flex-wrap gap-6px' role='radiogroup' aria-label='resolution'>
          {caps.resolutions.map((res) => {
            const active = res === currentRes;
            return (
              <button
                key={res}
                type='button'
                role='radio'
                aria-checked={active}
                disabled={disabled}
                onClick={(event) => {
                  event.preventDefault();
                  event.stopPropagation();
                  if (disabled || res === currentRes) return;
                  onChange({ resolution: res, fps: currentFps });
                }}
                className={[
                  'm-0 cursor-pointer border border-solid px-10px py-5px text-12px leading-18px transition-colors',
                  'rd-8px disabled:cursor-not-allowed disabled:opacity-50',
                  active
                    ? 'border-[rgb(var(--primary-6))] bg-[rgb(var(--primary-1))] text-[rgb(var(--primary-6))] font-600'
                    : 'border-[var(--color-border-2)] bg-[var(--color-bg-2)] text-[var(--color-text-2)] hover:border-[rgb(var(--primary-5))]',
                ].join(' ')}
              >
                {res}
              </button>
            );
          })}
        </div>
        <span className='text-11px text-[var(--color-text-4)]'>
          {t('videoGeneration.workspace.source.resolutionHint', {
            defaultValue: '可选档位随当前视频模型变化（Fast/Mini 最高 720p）',
          })}
        </span>
      </div>

      <div className='flex flex-col gap-6px text-12px text-[var(--color-text-3)]'>
        <span>
          {t('videoGeneration.workspace.source.fpsLabel', {
            defaultValue: '帧率',
          })}
        </span>
        <div className='flex flex-wrap gap-6px' role='radiogroup' aria-label='fps'>
          {caps.fpsOptions.map((fps) => {
            const active = fps === currentFps;
            const locked = caps.fpsLocked && caps.fpsOptions.length <= 1;
            return (
              <button
                key={fps}
                type='button'
                role='radio'
                aria-checked={active}
                disabled={disabled || locked}
                onClick={(event) => {
                  event.preventDefault();
                  event.stopPropagation();
                  if (disabled || locked || fps === currentFps) return;
                  onChange({ resolution: currentRes, fps });
                }}
                className={[
                  'm-0 cursor-pointer border border-solid px-10px py-5px text-12px leading-18px transition-colors',
                  'rd-8px disabled:cursor-not-allowed disabled:opacity-50',
                  active
                    ? 'border-[rgb(var(--primary-6))] bg-[rgb(var(--primary-1))] text-[rgb(var(--primary-6))] font-600'
                    : 'border-[var(--color-border-2)] bg-[var(--color-bg-2)] text-[var(--color-text-2)] hover:border-[rgb(var(--primary-5))]',
                ].join(' ')}
              >
                {fps} fps
              </button>
            );
          })}
        </div>
        <span className='text-11px text-[var(--color-text-4)]'>
          {caps.fpsLocked
            ? t('videoGeneration.workspace.source.fpsLockedHint', {
                defaultValue: '当前模型固定 {{fps}}fps（电影感帧率）',
                fps: currentFps,
              })
            : t('videoGeneration.workspace.source.fpsHint', {
                defaultValue: '可选帧率随当前视频模型变化',
              })}
        </span>
      </div>
    </div>
  );
};

export default VideoQualityPickers;
