/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * VideoGeneration workspace (`/video-generation/:sessionId`).
 *
 * Sections (one job each):
 * 1. Header — title + locked workflow badge + status
 * 2. Source input — idea / script / novel + Plan
 * 3. Artifacts — tree (left) + preview (right)
 * 4. Revise — target path + instruction
 * 5. Render + progress polling (2s while planning/rendering)
 * 6. Final video player when done
 */
import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import {
  Button,
  Input,
  InputNumber,
  Popconfirm,
  Result,
  Spin,
  Tag,
  Typography,
} from '@arco-design/web-react';
import { ArrowLeft, Delete, Play, Refresh, VideoOne } from '@icon-park/react';
import { useLayoutContext } from '@renderer/hooks/context/LayoutContext';
import { useArcoMessage } from '@renderer/utils/ui/useArcoMessage';
import {
  cancelSession,
  deleteSession,
  getArtifact,
  getSession,
  getSessionStatus,
  isActiveStatus,
  listArtifacts,
  loadArtifactMediaUrl,
  planSession,
  renderSession,
  reviseSession,
} from './api';
import type { ArtifactContent, ArtifactNode, SessionStatus, VimaxSession, VimaxWorkflow } from './types';
import ArtifactTree from './components/ArtifactTree';
import ModelSelectors, { type VimaxModelSelection } from './components/ModelSelectors';
import ProgressTimeline from './components/ProgressTimeline';
import { normalizeWorkflow, statusLabel, statusTagColor, workflowLabel } from './components/SessionCard';

const TextArea = Input.TextArea;

function sourceFieldForWorkflow(workflow: VimaxWorkflow | string): 'idea' | 'script' | 'novel_text' {
  switch (normalizeWorkflow(workflow)) {
    case 'script2video':
      return 'script';
    case 'novel2video':
      return 'novel_text';
    default:
      return 'idea';
  }
}

function isMediaPath(path: string): boolean {
  return /\.(png|jpe?g|gif|webp|bmp|mp4|webm|mov|avi|mkv)$/i.test(path);
}

function isVideoPath(path: string): boolean {
  return /\.(mp4|webm|mov|avi|mkv)$/i.test(path);
}

function collectMediaArtifacts(nodes: ArtifactNode[], acc: string[] = []): string[] {
  for (const n of nodes) {
    if (n.is_dir && n.children) {
      collectMediaArtifacts(n.children, acc);
    } else if (!n.is_dir && isMediaPath(n.path)) {
      // Prefer frames / shot clips / finals for the gallery strip.
      if (
        /first_frame|last_frame|video\.mp4|final_video/i.test(n.path) ||
        /\.(png|jpe?g|webp|mp4)$/i.test(n.path)
      ) {
        acc.push(n.path);
      }
    }
  }
  return acc
    .filter((p, i, arr) => arr.indexOf(p) === i)
    .sort((a, b) => a.localeCompare(b, undefined, { numeric: true }))
    .slice(0, 48);
}

const WorkspacePage: React.FC = () => {
  const { sessionId = '' } = useParams<{ sessionId: string }>();
  const { t } = useTranslation();
  const navigate = useNavigate();
  const layout = useLayoutContext();
  const isMobile = layout?.isMobile ?? false;
  const [message, messageHolder] = useArcoMessage();

  const [session, setSession] = useState<VimaxSession | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const [sourceText, setSourceText] = useState('');
  const [requirement, setRequirement] = useState('');
  const [style, setStyle] = useState('');
  const [targetDurationSecs, setTargetDurationSecs] = useState<number>(30);
  const [models, setModels] = useState<VimaxModelSelection>({
    llm_model: '',
    image_model: '',
    video_model: '',
  });

  const [planning, setPlanning] = useState(false);
  const [rendering, setRendering] = useState(false);
  const [revising, setRevising] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [deleting, setDeleting] = useState(false);

  const [runStatus, setRunStatus] = useState<SessionStatus | null>(null);
  const [artifacts, setArtifacts] = useState<ArtifactNode[]>([]);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [preview, setPreview] = useState<ArtifactContent | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);
  const [finalBlobUrl, setFinalBlobUrl] = useState<string | null>(null);

  const [reviseTarget, setReviseTarget] = useState('');
  const [reviseInstruction, setReviseInstruction] = useState('');

  const sourceField = session ? sourceFieldForWorkflow(session.workflow) : 'idea';

  const sourcePlaceholder = useMemo(() => {
    switch (sourceField) {
      case 'script':
        return t('videoGeneration.workspace.source.scriptPlaceholder', {
          defaultValue: '粘贴完整剧本…',
        });
      case 'novel_text':
        return t('videoGeneration.workspace.source.novelPlaceholder', {
          defaultValue: '粘贴小说文本…',
        });
      default:
        return t('videoGeneration.workspace.source.ideaPlaceholder', {
          defaultValue: '描述你的灵感或故事想法…',
        });
    }
  }, [sourceField, t]);

  const sourceLabel = useMemo(() => {
    switch (sourceField) {
      case 'script':
        return t('videoGeneration.workspace.source.scriptLabel', { defaultValue: '剧本' });
      case 'novel_text':
        return t('videoGeneration.workspace.source.novelLabel', { defaultValue: '小说文本' });
      default:
        return t('videoGeneration.workspace.source.ideaLabel', { defaultValue: '灵感' });
    }
  }, [sourceField, t]);

  const loadSession = useCallback(async () => {
    if (!sessionId) return;
    setLoading(true);
    try {
      const s = await getSession(sessionId);
      setSession(s);
      setSourceText(s.idea || s.script || s.novel_text || '');
      setRequirement(s.user_requirement || '');
      setStyle(s.style || '');
      setTargetDurationSecs(
        typeof s.target_duration_secs === 'number' && s.target_duration_secs > 0
          ? s.target_duration_secs
          : 30
      );
      setModels({
        llm_model: s.llm_model || '',
        image_model: s.image_model || '',
        video_model: s.video_model || '',
      });
      setLoadError(null);
    } catch (e) {
      setLoadError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, [sessionId]);

  const refreshArtifacts = useCallback(async () => {
    if (!sessionId) return;
    try {
      setArtifacts(await listArtifacts(sessionId));
    } catch (e) {
      console.warn('[videoGeneration] artifacts refresh failed', e);
    }
  }, [sessionId]);

  const refreshStatus = useCallback(async () => {
    if (!sessionId) return;
    try {
      const st = await getSessionStatus(sessionId);
      setRunStatus(st);
      if (st.status === 'succeeded' || st.final_video) {
        setSession((prev) =>
          prev
            ? { ...prev, status: st.status, stage: st.stage, final_video: st.final_video ?? prev.final_video }
            : prev
        );
      } else {
        setSession((prev) => (prev ? { ...prev, status: st.status, stage: st.stage } : prev));
      }
      return st;
    } catch (e) {
      console.warn('[videoGeneration] status poll failed', e);
      return null;
    }
  }, [sessionId]);

  useEffect(() => {
    void loadSession();
  }, [loadSession]);

  useEffect(() => {
    if (!sessionId || loading || loadError) return;
    void refreshArtifacts();
    void refreshStatus();
  }, [sessionId, loading, loadError, refreshArtifacts, refreshStatus]);

  // Poll while planning / rendering (1s so stage text feels live).
  // Also keep a slow poll while failed/idle so a late finish_job is not missed.
  useEffect(() => {
    if (!sessionId) return;
    const active = isActiveStatus(runStatus?.status);
    const ms = active ? 1000 : 5000;
    const timer = window.setInterval(() => {
      void (async () => {
        const st = await refreshStatus();
        if (st && !isActiveStatus(st.status)) {
          void refreshArtifacts();
        }
      })();
    }, ms);
    return () => window.clearInterval(timer);
  }, [runStatus?.status, sessionId, refreshStatus, refreshArtifacts]);

  // Load artifact preview when selection changes (blob URLs for media + auth).
  useEffect(() => {
    if (!sessionId || !selectedPath) {
      setPreview((prev) => {
        if (prev?.url?.startsWith('blob:')) URL.revokeObjectURL(prev.url);
        return null;
      });
      return;
    }
    let cancelled = false;
    setPreviewLoading(true);
    void getArtifact(sessionId, selectedPath)
      .then((content) => {
        if (cancelled) {
          if (content.url?.startsWith('blob:')) URL.revokeObjectURL(content.url);
          return;
        }
        setPreview((prev) => {
          if (prev?.url?.startsWith('blob:')) URL.revokeObjectURL(prev.url);
          return content;
        });
      })
      .catch((e) => {
        if (!cancelled) {
          setPreview({
            kind: 'text',
            text: e instanceof Error ? e.message : String(e),
          });
        }
      })
      .finally(() => {
        if (!cancelled) setPreviewLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [sessionId, selectedPath]);

  // Final video via authenticated blob URL (relative path is not a public HTTP URL).
  useEffect(() => {
    const rel = runStatus?.final_video || session?.final_video;
    if (!sessionId || !rel) {
      setFinalBlobUrl((prev) => {
        if (prev?.startsWith('blob:')) URL.revokeObjectURL(prev);
        return null;
      });
      return;
    }
    let cancelled = false;
    void loadArtifactMediaUrl(sessionId, rel)
      .then((url) => {
        if (cancelled) {
          URL.revokeObjectURL(url);
          return;
        }
        setFinalBlobUrl((prev) => {
          if (prev?.startsWith('blob:')) URL.revokeObjectURL(prev);
          return url;
        });
      })
      .catch((e) => {
        console.warn('[videoGeneration] final video load failed', e);
      });
    return () => {
      cancelled = true;
    };
  }, [sessionId, runStatus?.final_video, session?.final_video]);

  useEffect(() => {
    if (selectedPath) setReviseTarget(selectedPath);
  }, [selectedPath]);

  const handlePlan = useCallback(async () => {
    if (!sessionId || !session) return;
    const trimmed = sourceText.trim();
    if (!trimmed) {
      message.warning(
        t('videoGeneration.workspace.source.required', { defaultValue: '请先填写输入内容' })
      );
      return;
    }
    if (!models.llm_model.trim()) {
      message.warning(
        t('videoGeneration.workspace.models.llmRequired', {
          defaultValue: '请先选择规划模型（LLM）',
        })
      );
      return;
    }
    setPlanning(true);
    try {
      const body = {
        [sourceField]: trimmed,
        user_requirement: requirement.trim() || undefined,
        style: style.trim() || undefined,
        target_duration_secs: targetDurationSecs,
        llm_model: models.llm_model.trim() || undefined,
        image_model: models.image_model.trim() || undefined,
        video_model: models.video_model.trim() || undefined,
      };
      await planSession(sessionId, body);
      message.success(t('videoGeneration.workspace.planStarted', { defaultValue: '已开始规划' }));
      const st = await refreshStatus();
      if (!st || !isActiveStatus(st.status)) {
        // Optimistic: mark planning so polling kicks in even if status lags
        setRunStatus((prev) =>
          prev
            ? { ...prev, status: 'planning' }
            : { stage: 'plan', message: '', progress: 0, status: 'planning' }
        );
      }
      void refreshArtifacts();
    } catch (e) {
      message.error(
        `${t('videoGeneration.workspace.planFailed', { defaultValue: '规划失败' })}: ${
          e instanceof Error ? e.message : String(e)
        }`
      );
    } finally {
      setPlanning(false);
    }
  }, [
    sessionId,
    session,
    sourceText,
    sourceField,
    requirement,
    style,
    targetDurationSecs,
    models,
    message,
    t,
    refreshStatus,
    refreshArtifacts,
  ]);

  const handleRevise = useCallback(async () => {
    if (!sessionId) return;
    const target = reviseTarget.trim();
    const instruction = reviseInstruction.trim();
    if (!target || !instruction) {
      message.warning(
        t('videoGeneration.workspace.revise.required', {
          defaultValue: '请填写修订目标路径与说明',
        })
      );
      return;
    }
    setRevising(true);
    try {
      await reviseSession(sessionId, {
        revision_target: target,
        revision_instruction: instruction,
      });
      message.success(t('videoGeneration.workspace.revise.ok', { defaultValue: '已提交修订' }));
      setReviseInstruction('');
      const st = await refreshStatus();
      if (!st || !isActiveStatus(st.status)) {
        setRunStatus((prev) =>
          prev
            ? { ...prev, status: 'planning' }
            : { stage: 'revise', message: '', progress: 0, status: 'planning' }
        );
      }
      void refreshArtifacts();
    } catch (e) {
      message.error(
        `${t('videoGeneration.workspace.revise.failed', { defaultValue: '修订失败' })}: ${
          e instanceof Error ? e.message : String(e)
        }`
      );
    } finally {
      setRevising(false);
    }
  }, [sessionId, reviseTarget, reviseInstruction, message, t, refreshStatus, refreshArtifacts]);

  const handleRender = useCallback(async () => {
    if (!sessionId) return;
    if (!models.image_model.trim() || !models.video_model.trim()) {
      message.warning(
        t('videoGeneration.workspace.models.mediaRequired', {
          defaultValue: '请先选择图片模型与视频模型',
        })
      );
      return;
    }
    setRendering(true);
    try {
      await renderSession(sessionId, {
        llm_model: models.llm_model.trim() || undefined,
        image_model: models.image_model.trim() || undefined,
        video_model: models.video_model.trim() || undefined,
      });
      message.success(t('videoGeneration.workspace.renderStarted', { defaultValue: '已开始渲染' }));
      const st = await refreshStatus();
      if (!st || !isActiveStatus(st.status)) {
        setRunStatus((prev) =>
          prev
            ? { ...prev, status: 'rendering' }
            : { stage: 'render', message: '', progress: 0, status: 'rendering' }
        );
      }
    } catch (e) {
      message.error(
        `${t('videoGeneration.workspace.renderFailed', { defaultValue: '渲染失败' })}: ${
          e instanceof Error ? e.message : String(e)
        }`
      );
    } finally {
      setRendering(false);
    }
  }, [sessionId, models, message, t, refreshStatus]);

  const handleCancel = useCallback(async () => {
    if (!sessionId) return;
    setCancelling(true);
    try {
      await cancelSession(sessionId);
      message.info(t('videoGeneration.workspace.cancelOk', { defaultValue: '已请求取消' }));
      await refreshStatus();
    } catch (e) {
      message.error(
        `${t('videoGeneration.workspace.cancelFailed', { defaultValue: '取消失败' })}: ${
          e instanceof Error ? e.message : String(e)
        }`
      );
    } finally {
      setCancelling(false);
    }
  }, [sessionId, message, t, refreshStatus]);

  const handleDelete = useCallback(async () => {
    if (!sessionId || deleting) return;
    setDeleting(true);
    try {
      await deleteSession(sessionId);
      message.success(t('videoGeneration.actions.deleteOk', { defaultValue: '已删除任务' }));
      navigate('/video-generation');
    } catch (e) {
      message.error(
        `${t('videoGeneration.actions.deleteFailed', { defaultValue: '删除失败' })}: ${
          e instanceof Error ? e.message : String(e)
        }`
      );
      setDeleting(false);
    }
  }, [sessionId, deleting, message, t, navigate]);

  /** Prefer resume render when failure happened in a render-phase stage. */
  const continueAsRender = useMemo(() => {
    const events = runStatus?.events ?? [];
    const beforeFail = [...events].reverse().find((e) => e.stage && e.stage !== 'failed');
    const stage = beforeFail?.stage || runStatus?.stage || session?.stage || '';
    const renderStages = new Set([
      'render_start',
      'rendering',
      'render_scene',
      'render_resume',
      'render_scene_skip',
      'reuse_plan',
      'character_portraits_start',
      'frames_start',
      'frame_prompt_start',
      'video_clips_start',
      'concat_start',
      'video_generate',
      'image_generate',
    ]);
    return renderStages.has(stage) || stage.startsWith('render_');
  }, [runStatus, session?.stage]);

  const isFailed =
    (runStatus?.status ?? session?.status) === 'failed' ||
    (runStatus?.status ?? session?.status) === 'cancelled';

  const handleContinue = useCallback(() => {
    if (continueAsRender) {
      void handleRender();
    } else {
      void handlePlan();
    }
  }, [continueAsRender, handleRender, handlePlan]);

  const busy = isActiveStatus(runStatus?.status) || planning || rendering;
  const hasPlanned =
    artifacts.length > 0 ||
    session?.stage === 'planned' ||
    runStatus?.stage === 'planned' ||
    !!(runStatus?.stage || session?.stage) ||
    runStatus?.status === 'succeeded' ||
    session?.status === 'succeeded';
  const canRender = !busy && (hasPlanned || artifacts.length > 0 || isFailed);
  const canContinue = isFailed && !busy;
  const mediaGallery = useMemo(() => collectMediaArtifacts(artifacts), [artifacts]);

  const sectionClass =
    'flex flex-col gap-10px rd-12px border border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] p-16px';

  if (loading) {
    return (
      <div className='size-full flex items-center justify-center'>
        <Spin />
      </div>
    );
  }

  if (loadError || !session) {
    return (
      <div className='size-full flex items-center justify-center p-24px'>
        <Result
          status='error'
          title={t('videoGeneration.workspace.loadError', { defaultValue: '加载失败' })}
          subTitle={loadError ?? undefined}
          extra={
            <div className='flex gap-8px justify-center'>
              <Button onClick={() => navigate('/video-generation')}>
                {t('videoGeneration.workspace.back', { defaultValue: '返回列表' })}
              </Button>
              <Button type='primary' onClick={() => void loadSession()}>
                {t('videoGeneration.list.retry', { defaultValue: '重试' })}
              </Button>
            </div>
          }
        />
      </div>
    );
  }

  return (
    <div
      className={[
        'size-full box-border overflow-y-auto',
        isMobile ? 'px-12px py-12px' : 'px-16px py-20px md:px-32px md:py-24px',
      ].join(' ')}
    >
      {messageHolder}
      <div className='mx-auto flex w-full max-w-1280px box-border flex-col gap-14px'>
        {/* Header */}
        <div className='flex items-start justify-between gap-12px flex-wrap'>
          <div className='flex items-start gap-10px min-w-0'>
            <Button
              type='text'
              className='!px-6px shrink-0'
              onClick={() => navigate('/video-generation')}
              aria-label={t('videoGeneration.workspace.back', { defaultValue: '返回列表' })}
            >
              <ArrowLeft theme='outline' size={18} fill='currentColor' />
            </Button>
            <div className='min-w-0'>
              <div className='flex items-center gap-8px flex-wrap'>
                <h1 className='m-0 text-18px font-700 text-[var(--color-text-1)] truncate'>
                  {session.title || t('videoGeneration.list.untitled', { defaultValue: '未命名任务' })}
                </h1>
                <Tag size='small' color='arcoblue'>
                  {workflowLabel(session.workflow, t)}
                </Tag>
                <Tag size='small' color={statusTagColor(runStatus?.status ?? session.status)}>
                  {statusLabel(runStatus?.status ?? session.status, t)}
                </Tag>
              </div>
              <p className='m-0 mt-4px text-12px text-[var(--color-text-3)]'>
                {runStatus?.message ||
                  session?.stage ||
                  t('videoGeneration.workspace.workflowLocked', {
                    defaultValue: '工作流在创建后已锁定，不可更改。',
                  })}
              </p>
            </div>
          </div>
          <div className='flex items-center gap-8px shrink-0'>
            <Button
              type='outline'
              size='small'
              onClick={() => {
                void loadSession();
                void refreshArtifacts();
                void refreshStatus();
              }}
            >
              <span className='inline-flex items-center gap-4px'>
                <Refresh theme='outline' size={14} fill='currentColor' />
                {t('videoGeneration.workspace.refresh', { defaultValue: '刷新' })}
              </span>
            </Button>
            <Popconfirm
              title={t('videoGeneration.actions.deleteConfirm', {
                defaultValue: '确定删除该任务？产物将一并清除。',
              })}
              disabled={deleting}
              onOk={() => void handleDelete()}
            >
              <Button status='danger' type='outline' size='small' loading={deleting}>
                <span className='inline-flex items-center gap-4px'>
                  <Delete theme='outline' size={14} fill='currentColor' />
                  {t('videoGeneration.actions.delete', { defaultValue: '删除' })}
                </span>
              </Button>
            </Popconfirm>
          </div>
        </div>

        {/* Source input */}
        <section className={sectionClass}>
          <div className='flex items-center justify-between gap-8px'>
            <Typography.Text bold className='!text-14px'>
              {t('videoGeneration.workspace.source.title', { defaultValue: '输入素材' })}
            </Typography.Text>
            <Button type='primary' loading={planning} disabled={busy && !planning} onClick={() => void handlePlan()}>
              {isFailed && !continueAsRender
                ? t('videoGeneration.workspace.planContinue', { defaultValue: '从断点继续规划' })
                : t('videoGeneration.workspace.plan', { defaultValue: '开始规划' })}
            </Button>
          </div>
          <label className='text-12px text-[var(--color-text-3)]'>{sourceLabel}</label>
          <TextArea
            value={sourceText}
            onChange={setSourceText}
            placeholder={sourcePlaceholder}
            autoSize={{ minRows: 4, maxRows: 12 }}
            disabled={busy}
          />
          <div className={`grid gap-10px ${isMobile ? 'grid-cols-1' : 'grid-cols-3'}`}>
            <div className='flex flex-col gap-6px'>
              <label className='text-12px text-[var(--color-text-3)]'>
                {t('videoGeneration.workspace.source.durationLabel', {
                  defaultValue: '目标成片时长（秒）',
                })}
              </label>
              <InputNumber
                value={targetDurationSecs}
                onChange={(v) => setTargetDurationSecs(typeof v === 'number' ? v : 30)}
                min={5}
                max={180}
                step={5}
                disabled={busy}
                suffix='s'
                style={{ width: '100%' }}
              />
              <span className='text-11px text-[var(--color-text-3)]'>
                {t('videoGeneration.workspace.source.durationHint', {
                  defaultValue: '每个镜头视频至少 5 秒；规划会据此控制镜头数量。',
                })}
              </span>
            </div>
            <div className='flex flex-col gap-6px'>
              <label className='text-12px text-[var(--color-text-3)]'>
                {t('videoGeneration.workspace.source.requirementLabel', {
                  defaultValue: '额外要求（可选）',
                })}
              </label>
              <Input
                value={requirement}
                onChange={setRequirement}
                disabled={busy}
                placeholder={t('videoGeneration.workspace.source.requirementPlaceholder', {
                  defaultValue: '节奏、受众、风格偏好等',
                })}
              />
            </div>
            <div className='flex flex-col gap-6px'>
              <label className='text-12px text-[var(--color-text-3)]'>
                {t('videoGeneration.workspace.source.styleLabel', { defaultValue: '风格（可选）' })}
              </label>
              <Input
                value={style}
                onChange={setStyle}
                disabled={busy}
                placeholder={t('videoGeneration.workspace.source.stylePlaceholder', {
                  defaultValue: '如：赛博朋克、水墨、写实',
                })}
              />
            </div>
          </div>
          <div className='flex flex-col gap-8px mt-4px'>
            <label className='text-12px text-[var(--color-text-3)]'>
              {t('videoGeneration.workspace.models.title', {
                defaultValue: '模型（规划用 LLM，渲染用图片/视频）',
              })}
            </label>
            <ModelSelectors
              value={models}
              onChange={setModels}
              disabled={busy}
              isMobile={isMobile}
            />
          </div>
        </section>

        {/* Artifacts + preview */}
        <section className={sectionClass}>
          <Typography.Text bold className='!text-14px'>
            {t('videoGeneration.workspace.artifacts.title', { defaultValue: '产物' })}
          </Typography.Text>
          <div
            className={[
              'grid gap-12px min-h-240px',
              isMobile ? 'grid-cols-1' : 'grid-cols-[240px_1fr]',
            ].join(' ')}
          >
            <div className='rd-8px border border-solid border-[var(--color-border-2)] bg-[var(--color-fill-1)] overflow-hidden min-h-200px max-h-420px flex flex-col'>
              <div className='px-10px py-8px text-11px text-[var(--color-text-3)] border-b border-solid border-[var(--color-border-2)] border-l-0 border-r-0 border-t-0'>
                {t('videoGeneration.workspace.artifacts.tree', { defaultValue: '文件树' })}
              </div>
              <div className='flex-1 overflow-y-auto px-4px'>
                <ArtifactTree
                  tree={artifacts}
                  selectedPath={selectedPath}
                  onSelect={setSelectedPath}
                />
              </div>
            </div>
            <div className='rd-8px border border-solid border-[var(--color-border-2)] bg-[var(--color-fill-1)] min-h-200px max-h-420px overflow-hidden flex flex-col'>
              <div className='px-10px py-8px text-11px text-[var(--color-text-3)] border-b border-solid border-[var(--color-border-2)] border-l-0 border-r-0 border-t-0 truncate'>
                {selectedPath ??
                  t('videoGeneration.workspace.artifacts.selectHint', {
                    defaultValue: '选择左侧文件以预览',
                  })}
              </div>
              <div className='flex-1 overflow-auto p-12px'>
                {previewLoading ? (
                  <div className='flex justify-center py-40px'>
                    <Spin />
                  </div>
                ) : !selectedPath ? (
                  <div className='h-full flex items-center justify-center text-12px text-[var(--color-text-3)]'>
                    {t('videoGeneration.workspace.artifacts.selectHint', {
                      defaultValue: '选择左侧文件以预览',
                    })}
                  </div>
                ) : preview?.kind === 'url' && preview.url ? (
                  isVideoPath(selectedPath) || preview.mime?.startsWith('video/') ? (
                    <video src={preview.url} controls className='max-w-full max-h-360px rd-8px' />
                  ) : (
                    <img
                      src={preview.url}
                      alt={selectedPath}
                      className='max-w-full max-h-360px object-contain rd-8px'
                    />
                  )
                ) : preview?.text != null ? (
                  <pre className='m-0 whitespace-pre-wrap break-words text-12px leading-18px text-[var(--color-text-1)] font-mono'>
                    {preview.text}
                  </pre>
                ) : (
                  <div className='text-12px text-[var(--color-text-3)]'>
                    {t('videoGeneration.workspace.artifacts.previewEmpty', {
                      defaultValue: '无法预览此文件',
                    })}
                  </div>
                )}
              </div>
            </div>
          </div>
          {mediaGallery.length > 0 ? (
            <div className='mt-12px flex flex-col gap-8px'>
              <Typography.Text className='!text-12px !text-[var(--color-text-3)]'>
                {t('videoGeneration.workspace.artifacts.gallery', {
                  defaultValue: '关键帧 / 镜头片段快捷预览（点击在上方打开）',
                })}
              </Typography.Text>
              <div className='flex gap-8px overflow-x-auto pb-4px'>
                {mediaGallery.map((path) => (
                  <button
                    key={path}
                    type='button'
                    className={[
                      'shrink-0 max-w-160px px-8px py-6px rd-8px border border-solid text-11px text-left cursor-pointer',
                      selectedPath === path
                        ? 'border-[rgb(var(--primary-6))] bg-[var(--color-primary-light-1)] text-[var(--color-text-1)]'
                        : 'border-[var(--color-border-2)] bg-[var(--color-fill-1)] text-[var(--color-text-2)]',
                    ].join(' ')}
                    onClick={() => setSelectedPath(path)}
                    title={path}
                  >
                    {path.split('/').slice(-2).join('/')}
                  </button>
                ))}
              </div>
            </div>
          ) : null}
        </section>

        {/* Revise */}
        <section className={sectionClass}>
          <Typography.Text bold className='!text-14px'>
            {t('videoGeneration.workspace.revise.title', { defaultValue: '修订产物' })}
          </Typography.Text>
          <div className={`grid gap-10px ${isMobile ? 'grid-cols-1' : 'grid-cols-[1fr_2fr_auto]'}`}>
            <Input
              value={reviseTarget}
              onChange={setReviseTarget}
              disabled={busy}
              placeholder={t('videoGeneration.workspace.revise.targetPlaceholder', {
                defaultValue: '目标路径，如 storyboard/shot_01.json',
              })}
            />
            <Input
              value={reviseInstruction}
              onChange={setReviseInstruction}
              disabled={busy}
              placeholder={t('videoGeneration.workspace.revise.instructionPlaceholder', {
                defaultValue: '说明如何修改…',
              })}
            />
            <Button loading={revising} disabled={busy && !revising} onClick={() => void handleRevise()}>
              {t('videoGeneration.workspace.revise.submit', { defaultValue: '提交修订' })}
            </Button>
          </div>
        </section>

        {/* Progress + Render */}
        <section className={sectionClass}>
          <div className='flex items-center justify-between gap-8px flex-wrap'>
            <Typography.Text bold className='!text-14px'>
              {t('videoGeneration.workspace.progress.title', { defaultValue: '进度' })}
            </Typography.Text>
            <div className='flex items-center gap-8px flex-wrap'>
              {canContinue ? (
                <Button
                  type='primary'
                  status='warning'
                  loading={planning || rendering}
                  onClick={() => void handleContinue()}
                >
                  {t('videoGeneration.workspace.continue', {
                    defaultValue: '从断点继续',
                  })}
                </Button>
              ) : null}
              <Button
                type='primary'
                status='success'
                loading={rendering}
                disabled={!canRender || busy}
                onClick={() => void handleRender()}
              >
                <span className='inline-flex items-center gap-6px'>
                  <Play theme='outline' size={14} fill='currentColor' />
                  {isFailed && continueAsRender
                    ? t('videoGeneration.workspace.renderContinue', {
                        defaultValue: '从断点继续渲染',
                      })
                    : t('videoGeneration.workspace.render', { defaultValue: '开始渲染' })}
                </span>
              </Button>
            </div>
          </div>
          <ProgressTimeline
            status={runStatus}
            onCancel={() => void handleCancel()}
            cancelling={cancelling}
            models={models}
          />
        </section>

        {/* Final video */}
        {finalBlobUrl ? (
          <section className={sectionClass}>
            <div className='flex items-center gap-8px'>
              <VideoOne theme='outline' size={16} fill='currentColor' className='text-[rgb(var(--primary-6))]' />
              <Typography.Text bold className='!text-14px'>
                {t('videoGeneration.workspace.finalVideo', { defaultValue: '成片' })}
              </Typography.Text>
            </div>
            <video
              key={finalBlobUrl}
              src={finalBlobUrl}
              controls
              playsInline
              className='w-full max-h-480px rd-8px bg-black'
            />
          </section>
        ) : null}
      </div>
    </div>
  );
};

export default WorkspacePage;
