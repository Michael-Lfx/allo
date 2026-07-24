

import React, { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import type { TFunction } from 'i18next';
import { Progress, Button, Tag, Spin } from '@arco-design/web-react';
import type { SessionStatus, VimaxRunStatus } from '../types';
import { statusLabel, statusTagColor } from './SessionCard';
import { stageLabel } from '../stageI18n';

interface ProgressTimelineProps {
  status: SessionStatus | null;
  onCancel?: () => void;
  cancelling?: boolean;
  /** Currently selected models — used to explain failures. */
  models?: {
    llm_model?: string;
    image_model?: string;
    video_model?: string;
  };
}

const STEPS: { key: VimaxRunStatus; labelKey: string }[] = [
  { key: 'idle', labelKey: 'videoGeneration.status.idle' },
  { key: 'planning', labelKey: 'videoGeneration.status.planning' },
  { key: 'rendering', labelKey: 'videoGeneration.status.rendering' },
  { key: 'succeeded', labelKey: 'videoGeneration.status.succeeded' },
];

type FailureKind = 'llm' | 'image' | 'video' | 'unknown';

function classifyFailure(
  error: string,
  stage: string | null | undefined,
  events: SessionStatus['events'],
  t: TFunction
): { kind: FailureKind; title: string; hint: string } {
  const lower = error.toLowerCase();
  const isChannel = lower.includes('all channel models failed');

  // Prefer the stage just before "failed" in the event log.
  const beforeFail = [...(events ?? [])]
    .reverse()
    .find((e) => e.stage && e.stage !== 'failed');
  const stageKey = beforeFail?.stage || stage || '';

  const planningLlmStages = new Set([
    'planning',
    'develop_story',
    'extract_characters',
    'write_script',
    'plan_scene',
    'design_storyboard',
    'decompose_shots',
    'construct_camera_tree',
    'compress_novel',
    'compress_aggregate',
    'extract_events',
    'event_rag',
    'extract_scenes',
    'merge_characters',
  ]);
  const renderLlmStages = new Set([
    'reuse_plan',
    'render_start',
    'rendering',
    'render_scene',
    'render_resume',
    'frame_prompt_start',
  ]);
  const imageStages = new Set([
    'character_portraits_start',
    'world_assets_start',
    'frames_start',
    'frame_camera_start',
    'frame_start',
    'frame_prompt_start',
    'image_generate',
  ]);
  const videoStages = new Set(['video_clips_start', 'video_generate', 'concat_start']);

  let looksLikeLlm =
    lower.includes('llm failed') ||
    lower.includes('规划模型') ||
    lower.includes('聊天模型') ||
    lower.includes('chat_completions') ||
    lower.includes('empty content');

  const looksLikeBadImage =
    lower.includes('invalid png') ||
    lower.includes('open ref') ||
    lower.includes('decode image') ||
    lower.includes('downloaded image is not') ||
    (lower.includes('media processing') &&
      (lower.includes('.png') || lower.includes('three_view') || lower.includes('character_portrait')));

  let kind: FailureKind = 'unknown';
  if (looksLikeBadImage) {
    kind = 'image';
  } else if (looksLikeLlm || planningLlmStages.has(stageKey) || renderLlmStages.has(stageKey)) {
    kind = 'llm';
  } else if (lower.includes('image') || lower.includes('图片') || imageStages.has(stageKey)) {
    kind = 'image';
  } else if (
    lower.includes('video generation failed') ||
    lower.includes('视频生成') ||
    videoStages.has(stageKey)
  ) {
    kind = 'video';
  } else if (isChannel) {
    // Ambiguous channel failure: prefer LLM unless clearly in media stages.
    kind = 'llm';
  }

  const inRenderPhase = renderLlmStages.has(stageKey) || stageKey.startsWith('render_');

  if (kind === 'llm') {
    const isCameraTree = lower.includes('camera tree length mismatch');
    return {
      kind,
      title: inRenderPhase
        ? t('videoGeneration.workspace.failure.llmRenderTitle')
        : isCameraTree
          ? t('videoGeneration.workspace.failure.llmCameraTreeTitle')
          : t('videoGeneration.workspace.failure.llmPlanTitle'),
      hint: isCameraTree
        ? t('videoGeneration.workspace.failure.llmCameraTreeHint')
        : isChannel
          ? inRenderPhase
            ? t('videoGeneration.workspace.failure.llmChannelRenderHint')
            : t('videoGeneration.workspace.failure.llmChannelPlanHint')
          : inRenderPhase
            ? t('videoGeneration.workspace.failure.llmRenderHint')
            : t('videoGeneration.workspace.failure.llmPlanHint'),
    };
  }
  if (kind === 'image') {
    return {
      kind,
      title: t('videoGeneration.workspace.failure.imageTitle'),
      hint: isChannel
        ? t('videoGeneration.workspace.failure.imageChannelHint')
        : t('videoGeneration.workspace.failure.imageHint'),
    };
  }
  if (kind === 'video') {
    return {
      kind,
      title: t('videoGeneration.workspace.failure.videoTitle'),
      hint: isChannel
        ? t('videoGeneration.workspace.failure.videoChannelHint')
        : t('videoGeneration.workspace.failure.videoHint'),
    };
  }
  return {
    kind,
    title: t('videoGeneration.workspace.failure.unknownTitle'),
    hint: t('videoGeneration.workspace.failure.unknownHint'),
  };
}

function stepIndex(status: VimaxRunStatus | undefined, stage?: string | null): number {
  if (stage === 'planned' && (status === 'idle' || status === 'succeeded')) return 1;
  switch (status) {
    case 'planning':
      return 1;
    case 'rendering':
      return 2;
    case 'succeeded':
      return 3;
    case 'failed':
    case 'cancelled':
      return -1;
    default:
      return stage === 'planned' ? 1 : 0;
  }
}

const ProgressTimeline: React.FC<ProgressTimelineProps> = ({
  status,
  onCancel,
  cancelling,
  models,
}) => {
  const { t } = useTranslation();
  const events = useMemo(() => {
    const list = status?.events ?? [];
    return list.slice(-12).reverse();
  }, [status?.events]);

  const failure = useMemo(() => {
    if (!status?.error) return null;
    return classifyFailure(status.error, status.stage, status.events, t);
  }, [status?.error, status?.stage, status?.events, t]);

  const staleHint = useMemo(() => {
    if (!status || (status.status !== 'planning' && status.status !== 'rendering')) {
      return null;
    }
    const raw = status.updated_at || status.events?.[status.events.length - 1]?.at;
    if (!raw) return null;
    const ts = Date.parse(raw);
    if (Number.isNaN(ts)) return null;
    const ageSec = (Date.now() - ts) / 1000;
    if (ageSec < 90) return null;
    return t('videoGeneration.workspace.progress.stale');
  }, [status, t]);

  if (!status) {
    return (
      <div className='text-12px text-[var(--color-text-3)] py-8px'>
        {t('videoGeneration.workspace.progressIdle')}
      </div>
    );
  }

  const active = stepIndex(status.status, status.stage);
  const busy = status.status === 'planning' || status.status === 'rendering';
  const progress = Math.max(0, Math.min(100, Number(status.progress) || 0));
  const currentStage = stageLabel(status.stage, t);

  const relatedModel =
    failure?.kind === 'llm'
      ? models?.llm_model
      : failure?.kind === 'image'
        ? models?.image_model
        : failure?.kind === 'video'
          ? models?.video_model
          : undefined;

  return (
    <div className='flex flex-col gap-12px'>
      <div className='flex items-center justify-between gap-8px flex-wrap'>
        <div className='flex items-center gap-8px min-w-0'>
          <Tag size='small' color={statusTagColor(status.status)}>
            {statusLabel(status.status, t)}
          </Tag>
          {busy ? <Spin size={14} /> : null}
        </div>
        {busy && onCancel ? (
          <Button size='mini' status='danger' loading={cancelling} onClick={onCancel}>
            {t('videoGeneration.workspace.cancel')}
          </Button>
        ) : null}
      </div>

      <div
        className={[
          'rd-8px px-12px py-10px border border-solid',
          busy
            ? 'border-[rgba(var(--primary-6),0.35)] bg-[rgba(var(--primary-6),0.06)]'
            : 'border-[var(--color-border-2)] bg-[var(--color-fill-1)]',
        ].join(' ')}
      >
        <div className='text-11px text-[var(--color-text-3)] mb-2px'>
          {t('videoGeneration.workspace.progress.now')}
        </div>
        <div className='text-14px font-600 text-[var(--color-text-1)] leading-22px'>
          {currentStage ||
            (busy
              ? t('videoGeneration.workspace.progress.working')
              : t('videoGeneration.workspace.progress.idleStep'))}
        </div>
        {staleHint ? (
          <div className='text-12px leading-18px text-[rgb(var(--warning-6))] mt-6px'>
            {staleHint}
          </div>
        ) : null}
      </div>

      {status.working_dir_abs ? (
        <div className='text-11px text-[var(--color-text-3)] break-all'>
          {t('videoGeneration.workspace.progress.workdirLine', {
            path: status.working_dir_abs,
            defaultValue: '工作目录：{{path}}',
          })}
        </div>
      ) : null}

      {busy || progress > 0 ? (
        <Progress
          percent={busy && progress < 3 ? 3 : progress}
          animation={busy}
          showText
          size='small'
        />
      ) : null}

      {status.error && failure ? (
        <div className='rd-8px px-12px py-10px border border-solid border-[rgba(var(--danger-6),0.35)] bg-[rgba(var(--danger-6),0.06)] flex flex-col gap-6px'>
          <div className='text-13px font-600 text-[rgb(var(--danger-6))]'>{failure.title}</div>
          <div className='text-12px leading-18px text-[var(--color-text-1)]'>{failure.hint}</div>
          {relatedModel ? (
            <div className='text-11px text-[var(--color-text-3)]'>
              {t('videoGeneration.workspace.progress.currentModel', { model: relatedModel })}
            </div>
          ) : null}
          <details className='text-11px text-[var(--color-text-3)]'>
            <summary className='cursor-pointer select-none'>
              {t('videoGeneration.workspace.progress.errorDetail')}
            </summary>
            <pre className='m-0 mt-6px whitespace-pre-wrap break-all font-mono leading-16px text-[rgb(var(--danger-6))]'>
              {status.error}
            </pre>
          </details>
        </div>
      ) : null}

      <div className='flex items-center gap-0 overflow-x-auto'>
        {STEPS.map((step, i) => {
          const done = active >= 0 && i <= active;
          const current = active === i && busy;
          const plannedDone =
            step.key === 'planning' && status.stage === 'planned' && status.status === 'idle';
          return (
            <React.Fragment key={step.key}>
              {i > 0 && (
                <div
                  className='h-1px flex-1 min-w-12px'
                  style={{
                    background:
                      active >= 0 && i <= active
                        ? 'rgb(var(--primary-6))'
                        : 'var(--color-border-2)',
                  }}
                />
              )}
              <div className='flex flex-col items-center gap-4px shrink-0 px-4px'>
                <span
                  className={[
                    'w-8px h-8px rounded-full',
                    done || current || plannedDone
                      ? 'bg-[rgb(var(--primary-6))]'
                      : 'bg-[var(--color-fill-3)]',
                    current ? 'ring-2 ring-[rgba(var(--primary-6),0.35)]' : '',
                  ].join(' ')}
                />
                <span
                  className={[
                    'text-10px whitespace-nowrap',
                    current || plannedDone
                      ? 'text-[rgb(var(--primary-6))] font-600'
                      : 'text-[var(--color-text-3)]',
                  ].join(' ')}
                >
                  {t(step.labelKey)}
                </span>
              </div>
            </React.Fragment>
          );
        })}
      </div>

      {events.length > 0 ? (
        <div className='flex flex-col gap-4px'>
          <div className='text-11px text-[var(--color-text-3)]'>
            {t('videoGeneration.workspace.progress.log')}
          </div>
          <div className='max-h-160px overflow-y-auto rd-8px border border-solid border-[var(--color-border-2)] bg-[var(--color-fill-1)] px-10px py-8px flex flex-col gap-6px'>
            {events.map((ev, idx) => {
              const label = stageLabel(ev.stage, t);
              // Never show backend Chinese messages in the activity log — stage label is enough.
              return (
                <div key={`${ev.at}-${ev.stage}-${idx}`} className='flex gap-8px text-11px leading-16px'>
                  <span className='shrink-0 text-[var(--color-text-3)] tabular-nums'>
                    {formatEventTime(ev.at)}
                  </span>
                  <span
                    className={[
                      'shrink-0 font-500',
                      ev.stage === 'failed'
                        ? 'text-[rgb(var(--danger-6))]'
                        : 'text-[rgb(var(--primary-6))]',
                    ].join(' ')}
                  >
                    {label}
                  </span>
                </div>
              );
            })}
          </div>
        </div>
      ) : null}
    </div>
  );
};

function formatEventTime(at: string | undefined): string {
  if (!at) return '';
  const d = new Date(at);
  if (Number.isNaN(d.getTime())) return at.slice(11, 19) || at;
  return d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit', second: '2-digit' });
}

export default ProgressTimeline;
