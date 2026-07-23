

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
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useLocation, useNavigate, useParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import {
  Button,
  Input,
  InputNumber,
  Popconfirm,
  Result,
  Spin,
  Tag,
} from '@arco-design/web-react';
import { ArrowLeft, Delete, Download, Play, Refresh, VideoOne } from '@icon-park/react';
import { useLayoutContext } from '@renderer/hooks/context/LayoutContext';
import { useArcoMessage } from '@renderer/utils/ui/useArcoMessage';
import {
  confirmFirstValue,
  trackFunnelEvent,
} from '@renderer/utils/analytics/productFunnel';
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
import StoryboardBoard from './components/StoryboardBoard';
import StudioStageRail from './components/StudioStageRail';
import type { VideoCreateDraft } from './components/VideoCreateComposer';
import type { StoryboardScene } from './artifactPresentation';
import { findStoryboardPath } from './artifactPresentation';
import styles from './index.module.css';

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

function isVideoPath(path: string): boolean {
  return /\.(mp4|webm|mov|avi|mkv)$/i.test(path);
}

const WorkspacePage: React.FC = () => {
  const { sessionId = '' } = useParams<{ sessionId: string }>();
  const { t } = useTranslation();
  const navigate = useNavigate();
  const location = useLocation();
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
  const [revisionOpen, setRevisionOpen] = useState(false);
  const storyboardVisibleTracked = useRef(false);

  const launchDraft = (
    location.state as { launchDraft?: VideoCreateDraft; launchError?: boolean } | null
  )?.launchDraft;

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
      setSourceText(s.idea || s.script || s.novel_text || launchDraft?.sourceText || '');
      setRequirement(s.user_requirement || launchDraft?.requirement || '');
      setStyle(s.style || launchDraft?.style || '');
      setTargetDurationSecs(
        typeof s.target_duration_secs === 'number' && s.target_duration_secs > 0
          ? s.target_duration_secs
          : launchDraft?.targetDurationSecs ?? 30
      );
      setModels({
        llm_model: s.llm_model || launchDraft?.models.llm_model || '',
        image_model: s.image_model || launchDraft?.models.image_model || '',
        video_model: s.video_model || launchDraft?.models.video_model || '',
      });
      setLoadError(null);
    } catch (e) {
      setLoadError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, [launchDraft, sessionId]);

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
      confirmFirstValue({
        feature: 'video_generation',
        source: 'storyboard_revision',
        session_id: sessionId,
      });
      setReviseInstruction('');
      setRevisionOpen(false);
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

  const handleReviseScene = useCallback((scene: StoryboardScene) => {
    if (!scene.revisionPath) return;
    setReviseTarget(scene.revisionPath);
    setReviseInstruction('');
    setRevisionOpen(true);
  }, []);

  const handleDownload = useCallback(() => {
    if (!finalBlobUrl) return;
    confirmFirstValue({
      feature: 'video_generation',
      source: 'film_download',
      session_id: sessionId,
    });
    const anchor = document.createElement('a');
    anchor.href = finalBlobUrl;
    anchor.download = `${session?.title || 'nomi-video'}.mp4`;
    anchor.click();
  }, [finalBlobUrl, session?.title, sessionId]);

  const busy = isActiveStatus(runStatus?.status) || planning || rendering;
  const hasStoryboard =
    Boolean(findStoryboardPath(artifacts)) ||
    session?.stage === 'planned' ||
    runStatus?.stage === 'planned' ||
    runStatus?.status === 'rendering' ||
    runStatus?.status === 'succeeded' ||
    session?.status === 'succeeded';
  const canRender = !busy && (hasStoryboard || isFailed);
  const canContinue = isFailed && !busy;
  const currentStatus = runStatus?.status ?? session?.status;

  useEffect(() => {
    if (!hasStoryboard || storyboardVisibleTracked.current) return;
    storyboardVisibleTracked.current = true;
    trackFunnelEvent('first_artifact_visible', {
      feature: 'video_generation',
      artifact: 'storyboard',
      session_id: sessionId,
    });
  }, [hasStoryboard, sessionId]);

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
        styles.studioPage,
        'size-full box-border overflow-y-auto',
        isMobile ? 'px-12px py-12px' : 'px-16px py-20px md:px-32px md:py-24px',
      ].join(' ')}
    >
      {messageHolder}
      <div className='mx-auto flex w-full max-w-1180px box-border flex-col gap-14px'>
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

        <StudioStageRail
          status={currentStatus}
          stage={runStatus?.stage ?? session.stage}
          hasStoryboard={hasStoryboard}
          hasFinalVideo={Boolean(finalBlobUrl)}
        />

        {finalBlobUrl ? (
          <section className={`${styles.studioPanel} overflow-hidden`}>
            <div className='flex flex-wrap items-center justify-between gap-10px px-16px py-13px'>
              <div>
                <div className='flex items-center gap-7px text-14px font-650 text-[var(--color-text-1)]'>
                  <VideoOne
                    theme='outline'
                    size={16}
                    className='text-[rgb(var(--primary-6))]'
                  />
                  {t('videoGeneration.studio.filmReady', { defaultValue: '成片已就绪' })}
                </div>
                <div className='mt-2px text-11px text-[var(--color-text-3)]'>
                  {t('videoGeneration.studio.filmReadyHint', {
                    defaultValue: '播放检查，或下载到本地继续发布。',
                  })}
                </div>
              </div>
              <Button type='primary' onClick={handleDownload}>
                <span className='inline-flex items-center gap-6px'>
                  <Download theme='outline' size={14} />
                  {t('videoGeneration.studio.download', { defaultValue: '下载成片' })}
                </span>
              </Button>
            </div>
            <video
              key={finalBlobUrl}
              src={finalBlobUrl}
              controls
              playsInline
              onPlay={() =>
                confirmFirstValue({
                  feature: 'video_generation',
                  source: 'film_play',
                  session_id: sessionId,
                })
              }
              className='block w-full max-h-620px bg-black'
            />
          </section>
        ) : null}

        {!hasStoryboard ? (
          <section className={`${styles.studioPanel} p-16px md:p-20px`}>
            <div className='mb-14px flex flex-wrap items-start justify-between gap-10px'>
              <div>
                <h2 className='m-0 text-16px font-650 text-[var(--color-text-1)]'>
                  {t('videoGeneration.studio.briefTitle', { defaultValue: '把故事交给 Nomi' })}
                </h2>
                <p className='m-0 mt-3px text-12px text-[var(--color-text-3)]'>
                  {t('videoGeneration.studio.briefHint', {
                    defaultValue: '生成的是可修改分镜，不会直接开始高成本渲染。',
                  })}
                </p>
              </div>
              <Button
                type='primary'
                loading={planning}
                disabled={busy && !planning}
                onClick={() => void handlePlan()}
              >
                {isFailed && !continueAsRender
                  ? t('videoGeneration.workspace.planContinue', {
                      defaultValue: '从断点继续规划',
                    })
                  : t('videoGeneration.create.generateStoryboard', {
                      defaultValue: '生成分镜',
                    })}
              </Button>
            </div>
            <label className='mb-6px block text-12px text-[var(--color-text-3)]'>{sourceLabel}</label>
            <TextArea
              value={sourceText}
              onChange={setSourceText}
              placeholder={sourcePlaceholder}
              autoSize={{ minRows: 5, maxRows: 14 }}
              disabled={busy}
              className='!text-14px !leading-23px'
            />
            <div className={`mt-12px grid gap-10px ${isMobile ? 'grid-cols-1' : 'grid-cols-3'}`}>
              <label className='flex flex-col gap-6px text-12px text-[var(--color-text-3)]'>
                {t('videoGeneration.workspace.source.durationLabel', {
                  defaultValue: '目标成片时长（秒）',
                })}
                <InputNumber
                  value={targetDurationSecs}
                  onChange={(value) =>
                    setTargetDurationSecs(typeof value === 'number' ? value : 30)
                  }
                  min={5}
                  max={180}
                  step={5}
                  disabled={busy}
                  suffix='s'
                  style={{ width: '100%' }}
                />
              </label>
              <label className='flex flex-col gap-6px text-12px text-[var(--color-text-3)]'>
                {t('videoGeneration.workspace.source.requirementLabel', {
                  defaultValue: '额外要求（可选）',
                })}
                <Input
                  value={requirement}
                  onChange={setRequirement}
                  disabled={busy}
                  placeholder={t('videoGeneration.workspace.source.requirementPlaceholder', {
                    defaultValue: '节奏、受众、画幅等',
                  })}
                />
              </label>
              <label className='flex flex-col gap-6px text-12px text-[var(--color-text-3)]'>
                {t('videoGeneration.workspace.source.styleLabel', {
                  defaultValue: '视觉风格（可选）',
                })}
                <Input
                  value={style}
                  onChange={setStyle}
                  disabled={busy}
                  placeholder={t('videoGeneration.workspace.source.stylePlaceholder', {
                    defaultValue: '如：电影写实、复古胶片',
                  })}
                />
              </label>
            </div>
            <details className='mt-14px rd-10px bg-[var(--color-fill-1)] px-12px py-9px'>
              <summary className='cursor-pointer text-12px font-600 text-[var(--color-text-2)]'>
                {t('videoGeneration.studio.modelSettings', { defaultValue: '模型设置' })}
              </summary>
              <div className='mt-12px'>
                <ModelSelectors
                  value={models}
                  onChange={setModels}
                  disabled={busy}
                  isMobile={isMobile}
                />
              </div>
            </details>
          </section>
        ) : (
          <details className={`${styles.studioPanel} px-14px py-11px`}>
            <summary className='cursor-pointer text-12px font-600 text-[var(--color-text-2)]'>
              {t('videoGeneration.studio.briefAndModels', {
                defaultValue: '创意简报与模型设置',
              })}
            </summary>
            <div className='mt-12px flex flex-col gap-10px'>
              <TextArea
                value={sourceText}
                onChange={setSourceText}
                autoSize={{ minRows: 3, maxRows: 10 }}
                disabled={busy}
              />
              <ModelSelectors
                value={models}
                onChange={setModels}
                disabled={busy}
                isMobile={isMobile}
              />
            </div>
          </details>
        )}

        {runStatus && (busy || isFailed) ? (
          <section
            className={[
              styles.studioPanel,
              busy ? styles.progressGlow : '',
              'p-16px',
            ].join(' ')}
          >
            <ProgressTimeline
              status={runStatus}
              onCancel={() => void handleCancel()}
              cancelling={cancelling}
              models={models}
            />
          </section>
        ) : null}

        {hasStoryboard ? (
          <section className={`${styles.studioPanel} p-14px md:p-18px`}>
            <div className='mb-12px flex items-end justify-between gap-10px'>
              <div>
                <h2 className='m-0 text-16px font-650 text-[var(--color-text-1)]'>
                  {t('videoGeneration.studio.storyboard.title', { defaultValue: '故事分镜' })}
                </h2>
                <p className='m-0 mt-3px text-12px text-[var(--color-text-3)]'>
                  {t('videoGeneration.studio.storyboard.hint', {
                    defaultValue: '逐镜头检查叙事和画面，满意后再生成成片。',
                  })}
                </p>
              </div>
              <Tag size='small' color='arcoblue'>
                {t('videoGeneration.studio.storyboard.editable', { defaultValue: '可编辑' })}
              </Tag>
            </div>
            <StoryboardBoard
              sessionId={sessionId}
              artifacts={artifacts}
              disabled={busy}
              onReviseScene={handleReviseScene}
            />
          </section>
        ) : null}

        {revisionOpen ? (
          <section className={`${styles.studioPanel} p-16px`}>
            <div className='flex flex-wrap items-center justify-between gap-8px'>
              <div>
                <h3 className='m-0 text-14px font-650 text-[var(--color-text-1)]'>
                  {t('videoGeneration.studio.reviseTitle', { defaultValue: '想怎样修改这个镜头？' })}
                </h3>
                <p className='m-0 mt-3px text-11px text-[var(--color-text-3)]'>
                  {t('videoGeneration.studio.reviseHint', {
                    defaultValue: '直接描述结果，不需要填写技术文件路径。',
                  })}
                </p>
              </div>
              <Button type='text' size='small' onClick={() => setRevisionOpen(false)}>
                {t('common.cancel', { defaultValue: '取消' })}
              </Button>
            </div>
            <div className='mt-12px flex flex-col gap-9px md:flex-row'>
              <Input
                value={reviseInstruction}
                onChange={setReviseInstruction}
                disabled={busy}
                className='flex-1'
                placeholder={t('videoGeneration.studio.revisePlaceholder', {
                  defaultValue: '例如：让镜头更紧张，加入快速推镜和更强的雨势',
                })}
                onPressEnter={() => void handleRevise()}
              />
              <Button
                type='primary'
                loading={revising}
                disabled={busy || !reviseInstruction.trim()}
                onClick={() => void handleRevise()}
              >
                {t('videoGeneration.workspace.revise.submit', { defaultValue: '更新分镜' })}
              </Button>
            </div>
          </section>
        ) : null}

        {hasStoryboard ? (
          <section className={`${styles.studioPanel} flex flex-wrap items-center justify-between gap-14px p-16px`}>
            <div>
              <div className='text-14px font-650 text-[var(--color-text-1)]'>
                {t('videoGeneration.studio.renderTitle', { defaultValue: '分镜确认了吗？' })}
              </div>
              <div className='mt-3px text-12px text-[var(--color-text-3)]'>
                {t('videoGeneration.studio.renderHint', {
                  defaultValue: '渲染会生成关键帧、镜头视频并自动拼接成片。',
                })}
              </div>
            </div>
            <div className='flex flex-wrap items-center gap-8px'>
              {canContinue ? (
                <Button
                  type='primary'
                  status='warning'
                  loading={planning || rendering}
                  onClick={() => void handleContinue()}
                >
                  {t('videoGeneration.workspace.continue', { defaultValue: '从断点继续' })}
                </Button>
              ) : null}
              <Button
                type='primary'
                size='large'
                loading={rendering}
                disabled={!canRender || busy}
                onClick={() => void handleRender()}
              >
                <span className='inline-flex items-center gap-7px'>
                  <Play theme='outline' size={15} fill='currentColor' />
                  {isFailed && continueAsRender
                    ? t('videoGeneration.workspace.renderContinue', {
                        defaultValue: '继续生成成片',
                      })
                    : t('videoGeneration.studio.renderCta', { defaultValue: '生成成片' })}
                </span>
              </Button>
            </div>
          </section>
        ) : null}

        {artifacts.length > 0 ? (
          <details className={`${styles.studioPanel} px-14px py-11px`}>
            <summary className='cursor-pointer text-11px text-[var(--color-text-3)]'>
              {t('videoGeneration.studio.technicalDetails', { defaultValue: '技术产物与运行文件' })}
            </summary>
            <div
              className={[
                'mt-12px grid min-h-240px gap-12px',
                isMobile ? 'grid-cols-1' : 'grid-cols-[240px_1fr]',
              ].join(' ')}
            >
              <div className='max-h-420px min-h-200px overflow-hidden rd-8px border border-solid border-[var(--color-border-2)] bg-[var(--color-fill-1)]'>
                <ArtifactTree
                  tree={artifacts}
                  selectedPath={selectedPath}
                  onSelect={setSelectedPath}
                />
              </div>
              <div className='flex min-h-200px max-h-420px flex-col overflow-hidden rd-8px border border-solid border-[var(--color-border-2)] bg-[var(--color-fill-1)]'>
                <div className='truncate border-b border-l-0 border-r-0 border-t-0 border-solid border-[var(--color-border-2)] px-10px py-8px text-11px text-[var(--color-text-3)]'>
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
                  ) : preview?.kind === 'url' && preview.url && selectedPath ? (
                    isVideoPath(selectedPath) || preview.mime?.startsWith('video/') ? (
                      <video src={preview.url} controls className='max-h-360px max-w-full rd-8px' />
                    ) : (
                      <img
                        src={preview.url}
                        alt={selectedPath}
                        className='max-h-360px max-w-full rd-8px object-contain'
                      />
                    )
                  ) : preview?.text != null ? (
                    <pre className='m-0 whitespace-pre-wrap break-words font-mono text-12px leading-18px text-[var(--color-text-1)]'>
                      {preview.text}
                    </pre>
                  ) : (
                    <div className='text-12px text-[var(--color-text-3)]'>
                      {t('videoGeneration.workspace.artifacts.selectHint', {
                        defaultValue: '选择左侧文件以预览',
                      })}
                    </div>
                  )}
                </div>
              </div>
            </div>
          </details>
        ) : null}
      </div>
    </div>
  );
};

export default WorkspacePage;
