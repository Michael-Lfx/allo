

/**
 * VideoGeneration workspace (`/video-generation/:sessionId`).
 *
 * Sections (one job each):
 * 1. Header — title + locked workflow badge + status
 * 2. Technical artifacts — tree + editable preview (top)
 * 4. Active status (Planning / Rendering) — above the story brief
 * 5. Source input — idea / script / novel + Plan
 * 6. Render CTA — above storyboard once planned
 * 7. Storyboard — inline shot revise + filmstrip
 * 8. Final video player when done
 */
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useLocation, useNavigate, useParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import {
  Button,
  Input,
  Popconfirm,
  Result,
  Spin,
  Tag,
} from '@arco-design/web-react';
import { ArrowLeft, Delete, Export, Eyes, FolderOpen, Play, Refresh, Share, VideoOne, Cube } from '@icon-park/react';
import { ipcBridge } from '@/common';
import { useCloudAuth } from '@renderer/hooks/context/CloudAuthContext';
import { useLayoutContext } from '@renderer/hooks/context/LayoutContext';
import { useArcoMessage } from '@renderer/utils/ui/useArcoMessage';
import { isDesktopShell } from '@renderer/utils/platform';
import {
  confirmFirstValue,
  trackFunnelEvent,
} from '@renderer/utils/analytics/productFunnel';
import {
  cancelSession,
  deleteSession,
  exportSession,
  getArtifact,
  getSession,
  getSessionStatus,
  isActiveStatus,
  listArtifacts,
  loadArtifactMediaUrl,
  planSession,
  publishSessionToTvShow,
  renderSession,
  materializeSessionToCanvas,
  writeArtifactText,
} from './api';
import type { ArtifactContent, ArtifactNode, SessionStatus, VimaxSession, VimaxWorkflow } from './types';
import ArtifactTree from './components/ArtifactTree';
import ArtifactPreviewPanel from './components/ArtifactPreviewPanel';
import AspectRatioPicker from './components/AspectRatioPicker';
import DurationTimelineBar from './components/DurationTimelineBar';
import ModelSelectors, { type VimaxModelSelection } from './components/ModelSelectors';
import ProgressTimeline from './components/ProgressTimeline';
import VideoQualityPickers from './components/VideoQualityPickers';
import { normalizeWorkflow, statusLabel, statusTagColor, workflowLabel } from './components/SessionCard';
import StoryboardBoard from './components/StoryboardBoard';
import StudioStageRail from './components/StudioStageRail';
import VisualStyleSelect from './components/VisualStyleSelect';
import WorkspaceCameoStrip from './components/WorkspaceCameoStrip';
import type { VideoCreateDraft } from './components/VideoCreateComposer';
import type { StoryboardScene } from './artifactPresentation';
import { findStoryboardPath, patchShotDescriptionsInArtifact } from './artifactPresentation';
import { progressStatusText } from './stageI18n';
import {
  DEFAULT_SEEDANCE_ASPECT_RATIO,
  normalizeSeedanceAspectRatio,
} from './aspectRatios';
import {
  DEFAULT_VIDEO_FPS,
  DEFAULT_VIDEO_RESOLUTION,
  normalizeVideoFps,
  normalizeVideoResolution,
  type VideoResolution,
} from './videoModelCapabilities';
import { DEFAULT_VISUAL_STYLE_PROMPT } from './visualStylePresets';
import {
  clearVideoGenerationSessionMemory,
  rememberVideoGenerationSession,
} from './routeMemory';
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

const WorkspacePage: React.FC = () => {
  const { sessionId = '' } = useParams<{ sessionId: string }>();
  const { t } = useTranslation();
  const navigate = useNavigate();
  const location = useLocation();
  const layout = useLayoutContext();
  const isMobile = layout?.isMobile ?? false;
  const [message, messageHolder] = useArcoMessage();
  const { status: cloudStatus } = useCloudAuth();

  const [session, setSession] = useState<VimaxSession | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const [sourceText, setSourceText] = useState('');
  const [requirement, setRequirement] = useState('');
  const [style, setStyle] = useState(DEFAULT_VISUAL_STYLE_PROMPT);
  const [targetDurationSecs, setTargetDurationSecs] = useState<number>(30);
  const [aspectRatio, setAspectRatio] = useState(DEFAULT_SEEDANCE_ASPECT_RATIO);
  const [resolution, setResolution] = useState<VideoResolution>(DEFAULT_VIDEO_RESOLUTION);
  const [fps, setFps] = useState(DEFAULT_VIDEO_FPS);
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
  const [exporting, setExporting] = useState(false);
  const [publishing, setPublishing] = useState(false);
  const [materializing, setMaterializing] = useState(false);

  const [runStatus, setRunStatus] = useState<SessionStatus | null>(null);
  const [artifacts, setArtifacts] = useState<ArtifactNode[]>([]);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [preview, setPreview] = useState<ArtifactContent | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);
  /** Keep technical artifacts expanded by default so cast/env plates stay discoverable. */
  const [artifactsPanelOpen, setArtifactsPanelOpen] = useState(true);
  const [finalBlobUrl, setFinalBlobUrl] = useState<string | null>(null);
  const [coverBlobUrl, setCoverBlobUrl] = useState<string | null>(null);

  const [previewEpoch, setPreviewEpoch] = useState(0);
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
      rememberVideoGenerationSession(sessionId, s.title);
      setSourceText(s.idea || s.script || s.novel_text || launchDraft?.sourceText || '');
      setRequirement(s.user_requirement || launchDraft?.requirement || '');
      setStyle(s.style?.trim() || launchDraft?.style?.trim() || DEFAULT_VISUAL_STYLE_PROMPT);
      setTargetDurationSecs(
        typeof s.target_duration_secs === 'number' && s.target_duration_secs > 0
          ? s.target_duration_secs
          : launchDraft?.targetDurationSecs ?? 30
      );
      setAspectRatio(
        normalizeSeedanceAspectRatio(
          s.aspect_ratio || launchDraft?.aspectRatio || DEFAULT_SEEDANCE_ASPECT_RATIO
        )
      );
      const videoModel = s.video_model || launchDraft?.models.video_model || '';
      setModels({
        llm_model: s.llm_model || launchDraft?.models.llm_model || '',
        image_model: s.image_model || launchDraft?.models.image_model || '',
        video_model: videoModel,
      });
      setResolution(
        normalizeVideoResolution(
          videoModel,
          s.resolution || launchDraft?.resolution || DEFAULT_VIDEO_RESOLUTION
        )
      );
      setFps(
        normalizeVideoFps(
          videoModel,
          typeof s.fps === 'number' && s.fps > 0
            ? s.fps
            : launchDraft?.fps ?? DEFAULT_VIDEO_FPS
        )
      );
      setLoadError(null);
    } catch (e) {
      clearVideoGenerationSessionMemory(sessionId);
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
      if (st.status === 'succeeded' || st.final_video || st.cover) {
        setSession((prev) =>
          prev
            ? {
                ...prev,
                status: st.status,
                stage: st.stage,
                final_video: st.final_video ?? prev.final_video,
                cover: st.cover ?? prev.cover,
              }
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
  // Refresh artifacts whenever a clip lands — including repeated `video_clip_done`
  // for shot 2, 3, … (stage string alone is not unique across shots).
  const lastArtifactRefreshKeyRef = useRef<string | null>(null);
  const lastPeriodicArtifactAtRef = useRef(0);
  useEffect(() => {
    if (!sessionId) return;
    const active = isActiveStatus(runStatus?.status);
    const ms = active ? 1000 : 5000;
    const timer = window.setInterval(() => {
      void (async () => {
        const st = await refreshStatus();
        if (!st) return;
        if (!isActiveStatus(st.status)) {
          void refreshArtifacts();
          lastArtifactRefreshKeyRef.current = `${st.status}:${st.stage}:${st.updated_at ?? ''}`;
          return;
        }

        const stage = st.stage || '';
        const artifactLandingStages = new Set([
          'video_clip_done',
          'video_clip_exists',
          'video_download',
          'render_scene_done',
          'concat_done',
          'render_done',
        ]);
        // Include message + updated_at so consecutive shots finishing with the
        // same stage name still trigger a refresh.
        const refreshKey = `${stage}:${st.message ?? ''}:${st.updated_at ?? ''}`;
        if (
          artifactLandingStages.has(stage) &&
          refreshKey !== lastArtifactRefreshKeyRef.current
        ) {
          lastArtifactRefreshKeyRef.current = refreshKey;
          lastPeriodicArtifactAtRef.current = Date.now();
          void refreshArtifacts();
          return;
        }

        // Safety net: while rendering, rescan the artifact tree every ~4s so
        // storyboard thumbs catch videos even if a stage event was missed.
        const now = Date.now();
        if (now - lastPeriodicArtifactAtRef.current >= 4000) {
          lastPeriodicArtifactAtRef.current = now;
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
  }, [sessionId, selectedPath, previewEpoch]);

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

  // Film poster (display-only) via authenticated blob URL.
  useEffect(() => {
    const rel = runStatus?.cover || session?.cover;
    if (!sessionId || !rel) {
      setCoverBlobUrl((prev) => {
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
        setCoverBlobUrl((prev) => {
          if (prev?.startsWith('blob:')) URL.revokeObjectURL(prev);
          return url;
        });
      })
      .catch((e) => {
        console.warn('[videoGeneration] cover load failed', e);
      });
    return () => {
      cancelled = true;
    };
  }, [sessionId, runStatus?.cover, session?.cover]);

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
        aspect_ratio: aspectRatio,
        resolution,
        fps,
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
    aspectRatio,
    resolution,
    fps,
    models,
    message,
    t,
    refreshStatus,
    refreshArtifacts,
  ]);

  const handleSaveSceneDescriptions = useCallback(
    async (
      scene: StoryboardScene,
      descriptions: { visualDescription: string; audioDescription: string }
    ) => {
      if (!sessionId) return;
      const targetPath =
        scene.storyboardPath ||
        (scene.sceneRoot ? `${scene.sceneRoot.replace(/\\/g, '/')}/storyboard.json` : '') ||
        scene.revisionPath;
      if (!targetPath) {
        message.warning(
          t('videoGeneration.studio.storyboard.visualSaveMissing', {
            defaultValue: '找不到可保存的分镜文件',
          })
        );
        return;
      }
      setRevising(true);
      try {
        const current = await getArtifact(sessionId, targetPath);
        const patched = patchShotDescriptionsInArtifact(current.text, scene, descriptions);
        await writeArtifactText(sessionId, targetPath, patched);
        message.success(
          t('videoGeneration.studio.storyboard.visualSaveOk', {
            defaultValue: '画面描述已保存',
          })
        );
        confirmFirstValue({
          feature: 'video_generation',
          source: 'storyboard_revision',
          session_id: sessionId,
        });
        void refreshArtifacts();
        setPreviewEpoch((n) => n + 1);
      } catch (e) {
        message.error(
          `${t('videoGeneration.studio.storyboard.visualSaveFailed', {
            defaultValue: '保存画面描述失败',
          })}: ${e instanceof Error ? e.message : String(e)}`
        );
        throw e;
      } finally {
        setRevising(false);
      }
    },
    [sessionId, message, t, refreshArtifacts]
  );

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
        resolution,
        fps,
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
  }, [sessionId, models, resolution, fps, message, t, refreshStatus]);

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
      clearVideoGenerationSessionMemory(sessionId);
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

  const handleArtifactsChanged = useCallback(() => {
    void refreshArtifacts();
    setPreviewEpoch((n) => n + 1);
    message.success(
      t('videoGeneration.artifacts.saved', { defaultValue: '产物已更新' })
    );
  }, [message, refreshArtifacts, t]);

  const handleExportProject = useCallback(async () => {
    if (exporting || !sessionId) return;
    if (!isDesktopShell()) {
      message.info(
        t('videoGeneration.actions.exportDesktopOnly', {
          defaultValue: '导出工程仅桌面端可用。',
        })
      );
      return;
    }
    if (isActiveStatus(runStatus?.status) || planning || rendering) {
      message.warning(
        t('videoGeneration.actions.exportBusy', {
          defaultValue: '规划或渲染进行中，请完成后再导出。',
        })
      );
      return;
    }
    const safeTitle = (session?.title || 'nomi-video')
      .replace(/[\\/:*?"<>|\s]+/g, '-')
      .replace(/^-+|-+$/g, '')
      .slice(0, 48) || 'nomi-video';
    const dest = await ipcBridge.dialog.showSave.invoke({
      defaultPath: `${safeTitle}.nomivimax`,
      filters: [
        {
          name: t('videoGeneration.actions.exportFilter', { defaultValue: 'Flowy 视频工程' }),
          extensions: ['nomivimax'],
        },
      ],
    });
    if (!dest) return;
    setExporting(true);
    try {
      const result = await exportSession(sessionId, dest);
      message.success(
        `${t('videoGeneration.actions.exportOk', { defaultValue: '工程已导出' })}: ${result.dest_path}`
      );
      try {
        await ipcBridge.shell.showItemInFolder.invoke(result.dest_path);
      } catch {
        // Non-fatal: export already succeeded.
      }
    } catch (e) {
      message.error(
        `${t('videoGeneration.actions.exportFailed', { defaultValue: '导出失败' })}: ${
          e instanceof Error ? e.message : String(e)
        }`
      );
    } finally {
      setExporting(false);
    }
  }, [
    exporting,
    message,
    planning,
    rendering,
    runStatus?.status,
    session?.title,
    sessionId,
    t,
  ]);

  const handlePublishToTvShow = useCallback(async () => {
    if (publishing || !sessionId || !session) return;
    if (cloudStatus !== 'authenticated') {
      message.warning(
        t('videoGeneration.tvShow.authRequired.publish', {
          defaultValue: '发布到 TV Show 需要先登录云端账号。',
        })
      );
      navigate('/cloud-login');
      return;
    }
    const status = runStatus?.status ?? session.status;
    const hasFilm = Boolean(runStatus?.final_video || session.final_video);
    const hasCover = Boolean(runStatus?.cover || session.cover);
    if (status !== 'succeeded' || !hasFilm) {
      message.warning(
        t('videoGeneration.tvShow.publish.needSucceeded', {
          defaultValue: '请先完成成片生成后再发布。',
        })
      );
      return;
    }
    if (!hasCover) {
      message.warning(
        t('videoGeneration.tvShow.publish.needCover', {
          defaultValue: '缺少封面海报，请完成渲染后再发布。',
        })
      );
      return;
    }
    if (isActiveStatus(runStatus?.status) || planning || rendering) {
      message.warning(
        t('videoGeneration.tvShow.publish.busy', {
          defaultValue: '规划或渲染进行中，请完成后再发布。',
        })
      );
      return;
    }
    setPublishing(true);
    try {
      await publishSessionToTvShow(sessionId, {
        title: session.title || undefined,
      });
      message.success(
        t('videoGeneration.tvShow.publish.ok', {
          defaultValue: '已提交审核，通过后会出现在 TV Show 广场。',
        })
      );
    } catch (e) {
      message.error(
        `${t('videoGeneration.tvShow.publish.failed', { defaultValue: '发布失败' })}: ${
          e instanceof Error ? e.message : String(e)
        }`
      );
    } finally {
      setPublishing(false);
    }
  }, [
    cloudStatus,
    message,
    navigate,
    planning,
    publishing,
    rendering,
    runStatus?.cover,
    runStatus?.final_video,
    runStatus?.status,
    session,
    sessionId,
    t,
  ]);

  const handleRevealFilm = useCallback(async () => {
    const rel = (runStatus?.final_video || session?.final_video || '').replace(/\\/g, '/').replace(/^\/+/, '');
    const root = (runStatus?.working_dir_abs || '').replace(/\\/g, '/').replace(/\/+$/, '');
    if (!rel || !root) {
      message.warning(
        t('videoGeneration.studio.revealMissing', {
          defaultValue: '找不到成片本地路径，请刷新后重试。',
        })
      );
      return;
    }
    if (!isDesktopShell()) {
      message.info(
        t('videoGeneration.studio.revealDesktopOnly', {
          defaultValue: '打开视频所在位置仅桌面端可用。',
        })
      );
      return;
    }
    const abs = `${root}/${rel}`;
    confirmFirstValue({
      feature: 'video_generation',
      source: 'film_reveal',
      session_id: sessionId,
    });
    try {
      await ipcBridge.shell.showItemInFolder.invoke(abs);
    } catch (e) {
      message.error(
        `${t('videoGeneration.studio.revealFailed', { defaultValue: '无法打开视频所在位置' })}: ${
          e instanceof Error ? e.message : String(e)
        }`
      );
    }
  }, [message, runStatus?.final_video, runStatus?.working_dir_abs, session?.final_video, sessionId, t]);

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
  /** Plan finished (idle + `planned`) but the film is not rendered yet. */
  const plannedIdle =
    currentStatus === 'idle' &&
    !runStatus?.final_video &&
    !session?.final_video &&
    (runStatus?.stage === 'planned' || session?.stage === 'planned');
  const canPublishTvShow =
    !busy &&
    !publishing &&
    (runStatus?.status ?? session?.status) === 'succeeded' &&
    Boolean(runStatus?.final_video || session?.final_video) &&
    Boolean(runStatus?.cover || session?.cover);
  const canOpenInCanvas =
    !busy &&
    !materializing &&
    (hasStoryboard ||
      Boolean(runStatus?.final_video || session?.final_video) ||
      (runStatus?.status ?? session?.status) === 'succeeded');

  const handleOpenInCanvas = useCallback(async () => {
    if (!sessionId || materializing || busy) return;
    setMaterializing(true);
    try {
      const result = await materializeSessionToCanvas(sessionId);
      const warnText =
        result.warnings?.length > 0
          ? `（注意：${result.warnings.slice(0, 2).join('；')}${result.warnings.length > 2 ? '…' : ''}）`
          : '';
      message.success(
        t('videoGeneration.actions.openInCanvasOk', {
          defaultValue: '已打开到 Canvas：{{shots}} 镜 · {{media}} 个媒体',
          shots: result.shot_count,
          media: result.media_count,
        }) + warnText
      );
      navigate(`/video-generation/canvas/${encodeURIComponent(result.project_id)}`);
    } catch (e) {
      message.error(
        `${t('videoGeneration.actions.openInCanvasFailed', { defaultValue: '打开到 Canvas 失败' })}: ${
          e instanceof Error ? e.message : String(e)
        }`
      );
    } finally {
      setMaterializing(false);
    }
  }, [busy, materializing, message, navigate, sessionId, t]);

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
        'flex-1 min-h-0 size-full box-border overflow-y-auto',
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
                {progressStatusText(runStatus?.stage ?? session?.stage, runStatus?.message, t) ||
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
            <Button
              type='outline'
              size='small'
              loading={materializing}
              disabled={!canOpenInCanvas}
              onClick={() => void handleOpenInCanvas()}
            >
              <span className='inline-flex items-center gap-4px'>
                <Cube theme='outline' size={14} fill='currentColor' />
                {t('videoGeneration.actions.openInCanvas', { defaultValue: '打开到 Canvas' })}
              </span>
            </Button>
            <Button
              type='outline'
              size='small'
              loading={exporting}
              disabled={busy || exporting}
              onClick={() => void handleExportProject()}
            >
              <span className='inline-flex items-center gap-4px'>
                <Export theme='outline' size={14} fill='currentColor' />
                {t('videoGeneration.actions.exportProject', { defaultValue: '导出工程' })}
              </span>
            </Button>
            <Button
              type='outline'
              size='small'
              loading={publishing}
              disabled={!canPublishTvShow}
              onClick={() => void handlePublishToTvShow()}
            >
              <span className='inline-flex items-center gap-4px'>
                <Share theme='outline' size={14} fill='currentColor' />
                {t('videoGeneration.tvShow.publish.action', { defaultValue: '发布到 TV Show' })}
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

        {artifacts.length > 0 ? (
          <details
            className={`${styles.studioPanel} px-14px py-12px`}
            open={artifactsPanelOpen}
            onToggle={(event) => {
              setArtifactsPanelOpen((event.currentTarget as HTMLDetailsElement).open);
            }}
          >
            <summary className='cursor-pointer list-none marker:content-none'>
              <div className='flex flex-wrap items-center justify-between gap-8px'>
                <div>
                  <div className='text-14px font-650 text-[var(--color-text-1)]'>
                    {t('videoGeneration.studio.technicalDetails', {
                      defaultValue: '技术产物与运行文件',
                    })}
                  </div>
                  <div className='mt-2px text-12px text-[var(--color-text-3)]'>
                    {t('videoGeneration.studio.technicalDetailsHint', {
                      defaultValue: '审阅并编辑定妆图、环境/道具参考与工程文件，再生成成片。',
                    })}
                  </div>
                </div>
                <Tag size='small' color='orangered'>
                  {t('videoGeneration.studio.technicalDetailsBadge', {
                    defaultValue: '建议先检查',
                  })}
                </Tag>
              </div>
            </summary>
            <div
              className={[
                'mt-12px grid min-h-240px gap-12px',
                isMobile ? 'grid-cols-1' : 'grid-cols-[240px_1fr]',
              ].join(' ')}
            >
              <div className='flex max-h-420px min-h-200px flex-col overflow-hidden rd-8px border border-solid border-[var(--color-border-2)] bg-[var(--color-fill-1)]'>
                <ArtifactTree
                  tree={artifacts}
                  selectedPath={selectedPath}
                  onSelect={setSelectedPath}
                />
              </div>
              <ArtifactPreviewPanel
                sessionId={sessionId}
                selectedPath={selectedPath}
                preview={preview}
                previewLoading={previewLoading}
                disabled={busy}
                onChanged={handleArtifactsChanged}
                onRequestRegenerate={() => void handleRender()}
              />
            </div>
          </details>
        ) : null}

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
                    defaultValue: '播放检查，或打开视频所在文件夹。',
                  })}
                </div>
              </div>
              <Button type='primary' onClick={() => void handleRevealFilm()}>
                <span className='inline-flex items-center gap-6px'>
                  <FolderOpen theme='outline' size={14} />
                  {t('videoGeneration.studio.reveal', { defaultValue: '打开视频所在位置' })}
                </span>
              </Button>
            </div>
            <video
              key={finalBlobUrl}
              src={finalBlobUrl}
              poster={coverBlobUrl ?? undefined}
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

        {!hasStoryboard ? (
          <section className={`${styles.studioPanel} p-16px md:p-20px`}>
            <div className='mb-14px flex flex-wrap items-start justify-between gap-10px'>
              <div>
                <h2 className='m-0 text-16px font-650 text-[var(--color-text-1)]'>
                  {t('videoGeneration.studio.briefTitle', { defaultValue: '把故事交给 Flowy' })}
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
            {sessionId ? (
              <WorkspaceCameoStrip sessionId={sessionId} disabled={busy} />
            ) : null}
            <div className={`mt-12px grid gap-10px ${isMobile ? 'grid-cols-1' : 'grid-cols-2'}`}>
              <DurationTimelineBar
                wide
                value={targetDurationSecs}
                disabled={busy}
                onChange={setTargetDurationSecs}
              />
              <div className='flex flex-col gap-6px text-12px text-[var(--color-text-3)]'>
                <span>
                  {t('videoGeneration.workspace.source.aspectLabel', {
                    defaultValue: '视频比例',
                  })}
                </span>
                <AspectRatioPicker
                  value={aspectRatio}
                  onChange={setAspectRatio}
                  disabled={busy}
                />
                <span className='text-11px text-[var(--color-text-4)]'>
                  {t('videoGeneration.workspace.source.aspectHint', {
                    defaultValue: '同时作用于 Seedance 成片与海报封面',
                  })}
                </span>
              </div>
              <label className='flex flex-col gap-6px text-12px text-[var(--color-text-3)] md:col-span-2'>
                {t('videoGeneration.workspace.source.requirementLabel', {
                  defaultValue: '额外要求（可选）',
                })}
                <Input
                  value={requirement}
                  onChange={setRequirement}
                  disabled={busy}
                  placeholder={t('videoGeneration.workspace.source.requirementPlaceholder', {
                    defaultValue: '节奏、受众等（画幅请在上方比例中选择）',
                  })}
                />
              </label>
              <div
                className={`flex flex-col gap-6px text-12px text-[var(--color-text-3)] ${
                  isMobile ? '' : 'col-span-2'
                }`}
              >
                <span>
                  {t('videoGeneration.workspace.source.styleLabel', {
                    defaultValue: '视觉风格（人物与成片）',
                  })}
                </span>
                <VisualStyleSelect value={style} onChange={setStyle} disabled={busy} />
                <span className='text-11px text-[var(--color-text-4)]'>
                  {t('videoGeneration.workspace.source.styleHint', {
                    defaultValue:
                      '定妆为单张三视图；面部轻微柔化但五官清晰。规划阶段也会生成全局环境与道具参考图。',
                  })}
                </span>
              </div>
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
                <div className='mt-12px'>
                  <VideoQualityPickers
                    videoModel={models.video_model}
                    value={{ resolution, fps }}
                    onChange={({ resolution: nextRes, fps: nextFps }) => {
                      setResolution(nextRes);
                      setFps(nextFps);
                    }}
                    disabled={busy}
                  />
                </div>
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
              <div className={`grid gap-10px ${isMobile ? 'grid-cols-1' : 'grid-cols-2'}`}>
                <DurationTimelineBar wide value={targetDurationSecs} disabled />
                <div className='flex flex-col gap-6px text-12px text-[var(--color-text-3)]'>
                  <span>
                    {t('videoGeneration.workspace.source.aspectLabel', {
                      defaultValue: '视频比例',
                    })}
                  </span>
                  <AspectRatioPicker value={aspectRatio} onChange={setAspectRatio} disabled />
                  <span className='text-11px text-[var(--color-text-4)]'>
                    {t('videoGeneration.workspace.source.aspectLockedHint', {
                      defaultValue: '规划后画幅已锁定，同时作用于成片与海报封面',
                    })}
                  </span>
                </div>
              </div>
              <ModelSelectors
                value={models}
                onChange={setModels}
                disabled={busy}
                isMobile={isMobile}
              />
              <VideoQualityPickers
                videoModel={models.video_model}
                value={{ resolution, fps }}
                onChange={({ resolution: nextRes, fps: nextFps }) => {
                  setResolution(nextRes);
                  setFps(nextFps);
                }}
                disabled={busy}
              />
            </div>
          </details>
        )}

        {hasStoryboard ? (
          <section className={`${styles.studioPanel} flex flex-wrap items-center justify-between gap-14px p-16px`}>
            {plannedIdle ? (
              <div className='w-full rd-8px px-12px py-10px border border-solid border-[rgba(var(--primary-6),0.35)] bg-[rgba(var(--primary-6),0.06)]'>
                <div className='flex items-center gap-6px text-13px font-600 text-[var(--color-text-1)]'>
                  <Eyes theme='outline' size={15} className='text-[rgb(var(--primary-6))]' />
                  {t('videoGeneration.studio.portraitReviewTitle', {
                    defaultValue: '规划完成——渲染前可先审阅定妆图',
                  })}
                </div>
                <div className='mt-2px text-12px leading-18px text-[var(--color-text-3)]'>
                  {t('videoGeneration.studio.portraitReviewHint', {
                    defaultValue:
                      '规划阶段已生成全局角色定妆图与环境/道具参考图。建议先在上方「技术产物与运行文件」中检查它们，满意后再生成成片（高成本、不可逆）。',
                  })}
                </div>
              </div>
            ) : null}
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
              runStatus={runStatus}
              disabled={busy}
              revising={revising}
              onSaveSceneDescriptions={handleSaveSceneDescriptions}
            />
          </section>
        ) : null}
      </div>
    </div>
  );
};

export default WorkspacePage;
