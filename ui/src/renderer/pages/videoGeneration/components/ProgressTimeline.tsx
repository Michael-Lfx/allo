/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { Progress, Button, Tag, Spin } from '@arco-design/web-react';
import type { SessionStatus, VimaxRunStatus } from '../types';
import { statusLabel, statusTagColor } from './SessionCard';

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

const STEPS: { key: VimaxRunStatus; labelKey: string; labelDefault: string }[] = [
  { key: 'idle', labelKey: 'videoGeneration.status.idle', labelDefault: '待命' },
  { key: 'planning', labelKey: 'videoGeneration.status.planning', labelDefault: '规划中' },
  { key: 'rendering', labelKey: 'videoGeneration.status.rendering', labelDefault: '渲染中' },
  { key: 'succeeded', labelKey: 'videoGeneration.status.succeeded', labelDefault: '已完成' },
];

/** Human-readable labels for pipeline stage keys. */
const STAGE_LABELS: Record<string, string> = {
  planning: '准备规划',
  rendering: '准备渲染',
  save_novel: '保存并切分小说',
  compress_novel: '压缩小说分片',
  compress_aggregate: '汇总压缩结果',
  extract_events: '提取事件',
  event_rag: '检索相关片段',
  extract_scenes: '提取场景',
  merge_characters: '合并角色信息',
  plan_scene: '规划场景产物',
  develop_story: '扩写故事',
  extract_characters: '提取角色',
  write_script: '撰写剧本',
  design_storyboard: '设计分镜',
  decompose_shots: '分解镜头',
  construct_camera_tree: '构建机位树',
  planned: '规划完成',
  reuse_plan: '复用规划产物',
  character_portraits_start: '生成角色定妆图',
  render_start: '开始渲染',
  render_scene: '渲染场景',
  render_scene_skip: '跳过已完成场景',
  render_resume: '从断点继续渲染',
  frames_start: '生成关键帧',
  frame_prompt_start: '选择参考图并生成提示',
  video_clips_start: '生成视频片段',
  video_clip_exists: '跳过已有视频',
  video_clip_start: '生成镜头视频',
  video_clip_done: '镜头视频已保存',
  video_clips_partial: '部分镜头视频失败',
  video_clips_done: '镜头视频全部完成',
  concat_start: '拼接成片',
  concat_done: '拼接完成',
  render_done: '渲染完成',
  final_video_exists: '成片已存在',
  failed: '失败',
  cancelled: '已取消',
};

function stageLabel(stage: string | null | undefined): string {
  if (!stage) return '';
  return STAGE_LABELS[stage] ?? stage;
}

type FailureKind = 'llm' | 'image' | 'video' | 'unknown';

function classifyFailure(
  error: string,
  stage: string | null | undefined,
  events: SessionStatus['events']
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
  const imageStages = new Set(['character_portraits_start', 'frames_start', 'image_generate']);
  const videoStages = new Set(['video_clips_start', 'video_generate', 'concat_start']);

  const looksLikeLlm =
    lower.includes('llm failed') ||
    lower.includes('规划模型') ||
    lower.includes('聊天模型') ||
    lower.includes('chat_completions') ||
    lower.includes('empty content');

  let kind: FailureKind = 'unknown';
  if (looksLikeLlm || planningLlmStages.has(stageKey) || renderLlmStages.has(stageKey)) {
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
        ? '渲染阶段聊天模型（LLM）调用失败'
        : isCameraTree
          ? '机位树规划结果不完整'
          : '规划模型（LLM）调用失败',
      hint: isCameraTree
        ? '模型返回的机位父子关系条数与镜头机位数量不一致。已支持自动补齐；请重启后端后点「从断点继续」。若仍失败可换规划模型。'
        : isChannel
        ? inRenderPhase
          ? '这不是视频生成模型失败。渲染过程中的分镜/参考图选择也需要聊天模型。请更换「规划模型（LLM）」后点击「从断点继续」。'
          : '这不是视频生成模型失败。Flowy 上游表示当前所选聊天模型的通道全部不可用。请更换「规划模型（LLM）」后重新点「开始规划」或「从断点继续」。'
        : inRenderPhase
          ? '渲染过程中调用聊天模型失败（如分镜提示、参考图选择）。已保留现场产物，请更换模型或稍后点击「从断点继续」。'
          : '文本规划阶段调用聊天模型失败。请更换规划模型，或稍后点击「从断点继续」。',
    };
  }
  if (kind === 'image') {
    return {
      kind,
      title: '图片模型调用失败',
      hint: isChannel
        ? '生成定妆图/关键帧时图片通道不可用。请更换「图片模型」后点击「从断点继续」。'
        : '请检查图片模型是否可用。已保留现场，可点击「从断点继续」。',
    };
  }
  if (kind === 'video') {
    return {
      kind,
      title: '视频模型调用失败',
      hint: isChannel
        ? '生成镜头成片时视频通道不可用。请更换「视频模型」后点击「从断点继续」。'
        : '请检查视频模型是否可用。已保留现场，可点击「从断点继续」。',
    };
  }
  return {
    kind,
    title: '执行失败',
    hint: '现场产物已保留。若错误含 All channel models failed，请更换对应模型后点击「从断点继续」。',
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
    return classifyFailure(status.error, status.stage, status.events);
  }, [status?.error, status?.stage, status?.events]);

  if (!status) {
    return (
      <div className='text-12px text-[var(--color-text-3)] py-8px'>
        {t('videoGeneration.workspace.progressIdle', {
          defaultValue: '尚未开始。提交规划后将显示进度。',
        })}
      </div>
    );
  }

  const active = stepIndex(status.status, status.stage);
  const busy = status.status === 'planning' || status.status === 'rendering';
  const progress = Math.max(0, Math.min(100, Number(status.progress) || 0));
  const currentStage = stageLabel(status.stage);
  const currentMessage = status.message?.trim() || '';

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
            {t('videoGeneration.workspace.cancel', { defaultValue: '取消' })}
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
          {t('videoGeneration.workspace.progress.now', { defaultValue: '当前步骤' })}
        </div>
        <div className='text-14px font-600 text-[var(--color-text-1)] leading-22px'>
          {currentStage ||
            (busy
              ? t('videoGeneration.workspace.progress.working', { defaultValue: '处理中…' })
              : t('videoGeneration.workspace.progress.idleStep', { defaultValue: '待命' }))}
        </div>
        {currentMessage && !status.error ? (
          <div className='text-12px leading-18px text-[var(--color-text-2)] mt-4px'>
            {currentMessage}
          </div>
        ) : null}
      </div>

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
              当前选用：{relatedModel}
            </div>
          ) : null}
          <details className='text-11px text-[var(--color-text-3)]'>
            <summary className='cursor-pointer select-none'>
              {t('videoGeneration.workspace.progress.errorDetail', {
                defaultValue: '查看原始错误',
              })}
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
                  {t(step.labelKey, { defaultValue: step.labelDefault })}
                </span>
              </div>
            </React.Fragment>
          );
        })}
      </div>

      {events.length > 0 ? (
        <div className='flex flex-col gap-4px'>
          <div className='text-11px text-[var(--color-text-3)]'>
            {t('videoGeneration.workspace.progress.log', { defaultValue: '最近动态' })}
          </div>
          <div className='max-h-160px overflow-y-auto rd-8px border border-solid border-[var(--color-border-2)] bg-[var(--color-fill-1)] px-10px py-8px flex flex-col gap-6px'>
            {events.map((ev, idx) => (
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
                  {stageLabel(ev.stage)}
                </span>
                <span className='min-w-0 text-[var(--color-text-2)] break-all line-clamp-3'>
                  {ev.message}
                </span>
              </div>
            ))}
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
