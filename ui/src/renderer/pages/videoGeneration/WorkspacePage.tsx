

/**
 * VideoGeneration workspace (`/video-generation/:sessionId`).
 *
 * Layout: sticky header + scrolling artifact column + docked Agent session.
 * Pipeline actions (plan / render / cancel / continue) live in the session composer.
 */
import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
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
import { ArrowLeft, Delete, Export, FolderOpen, Refresh, Share, VideoOne, Cube, Robot } from '@icon-park/react';
import { ipcBridge } from '@/common';
import { isInvalidCloudSessionError } from '@/common/adapter/httpBridge';
import { useCloudAuth } from '@renderer/hooks/context/CloudAuthContext';
import { useLayoutContext } from '@renderer/hooks/context/LayoutContext';
import { useArcoMessage } from '@renderer/utils/ui/useArcoMessage';
import { isDesktopShell } from '@renderer/utils/platform';
import {
  confirmFirstValue,
  hasVideoSessionEvent,
  trackFunnelEvent,
  trackVideoSessionEvent,
} from '@renderer/utils/analytics/productFunnel';
import {
  cancelSession,
  deleteSession,
  exportSession,
  getArtifact,
  getSession,
  isActiveStatus,
  listArtifacts,
  loadArtifactMediaUrl,
  planSession,
  publishSessionToTvShow,
  renderSession,
  materializeSessionToCanvas,
  writeArtifactText,
  listCameos,
  uploadCameo,
  updateSessionTitle,
} from './api';
import type { ArtifactContent, ArtifactNode, VimaxSession, VimaxWorkflow } from './types';
import ArtifactTree from './components/ArtifactTree';
import ArtifactPreviewPanel from './components/ArtifactPreviewPanel';
import AspectRatioPicker from './components/AspectRatioPicker';
import ModelSelectors, { type VimaxModelSelection } from './components/ModelSelectors';
import VideoQualityPickers from './components/VideoQualityPickers';
import { normalizeWorkflow, isActionImitationWorkflow, statusLabel, workflowLabel } from './components/SessionCard';
import StoryboardBoard from './components/StoryboardBoard';
import VisualStyleSelect from './components/VisualStyleSelect';
import WorkspaceActionAssets from './components/WorkspaceActionAssets';
import type { VideoCreateDraft } from './home/types';
import type { StoryboardScene, StoryboardSceneSave } from './artifactPresentation';
import {
  findStoryboardPath,
  patchShotDescriptionsInArtifact,
} from './artifactPresentation';
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
} from '@renderer/services/videoModelCapabilities';
import {
  clearVideoGenerationSessionMemory,
  rememberVideoGenerationSession,
  updateRecentVideoGenerationTitle,
} from './routeMemory';
import { isInsufficientCreditsError } from './creditsError';
import { filmTelemetryError } from './providerError';
import { resolveSessionCreditsConsumed } from './sessionCredits';
import { shouldContinueAsRender } from './continueMode';
import {
  getRunStatusSnapshot,
  patchRunStatus,
  useRunStatusFeedController,
  useRunStatusFlags,
  useRunStatusFull,
} from './useRunStatusFeed';
import StudioAgentSession from './studioAgentSession/StudioAgentSession';
import {
  computeStudioSessionWidth,
  loadStudioSessionCollapsed,
  loadStudioSessionWidthRatio,
  saveStudioSessionCollapsed,
  saveStudioSessionWidthRatio,
} from './studioAgentSession/sessionPanelStorage';
import { clampDuration } from './durationBounds';
import styles from './index.module.css';
import { CanvasChromeButton } from '@oc/components/canvas/canvas-overlay';
import WorkspaceTitleField from './components/WorkspaceTitleField';
import '@oc/styles/quiet-chrome.css';
import { loadVideoCanvasProjectPage } from '../videoCanvas/loadProjectPage';
import { videoCanvasProjectPath } from '../videoCanvas/routes';

const TextArea = Input.TextArea;

/** Dedupes home→workspace auto-plan across React Strict Mode remounts. */
const autoPlannedSessions = new Set<string>();

function sourceDocumentStorageKey(sessionId: string): string {
  return `vimax-source-document:${sessionId}`;
}

function readStoredSourceDocument(sessionId: string): string | null {
  if (!sessionId || typeof sessionStorage === 'undefined') return null;
  try {
    const value = sessionStorage.getItem(sourceDocumentStorageKey(sessionId));
    return value?.trim() || null;
  } catch {
    return null;
  }
}

function storeSourceDocument(sessionId: string, name: string): void {
  if (!sessionId || typeof sessionStorage === 'undefined') return;
  try {
    sessionStorage.setItem(sourceDocumentStorageKey(sessionId), name);
  } catch {
    // ignore quota / private mode
  }
}

type WorkspaceLaunchState = {
  launchDraft?: VideoCreateDraft;
  launchError?: boolean;
  autoPlan?: boolean;
};

function sourceFieldForWorkflow(workflow: VimaxWorkflow | string): 'idea' | 'script' | 'novel_text' {
  switch (normalizeWorkflow(workflow)) {
    case 'script2video':
      return 'script';
    case 'novel2video':
      return 'novel_text';
    case 'action2video':
    case 'idea2video':
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
  const { status: cloudStatus, logout } = useCloudAuth();

  const [session, setSession] = useState<VimaxSession | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const [sourceText, setSourceText] = useState('');
  const [requirement, setRequirement] = useState('');
  const [style, setStyle] = useState('');
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

  const [artifacts, setArtifacts] = useState<ArtifactNode[]>([]);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [preview, setPreview] = useState<ArtifactContent | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);
  /** Keep technical artifacts expanded by default so cast/env plates stay discoverable. */
  const [artifactsPanelOpen, setArtifactsPanelOpen] = useState(true);
  const [finalBlobUrl, setFinalBlobUrl] = useState<string | null>(null);
  const [coverBlobUrl, setCoverBlobUrl] = useState<string | null>(null);

  // 卸载兜底：替换/取消路径各自 revoke，但 SPA 内离开页面不会卸载文档，
  // 不补这一步的话每次进出已完成工作台都泄漏成片/封面/预览的 blob。
  const heldBlobUrlsRef = useRef<{ final: string | null; cover: string | null; preview: string | null }>({ final: null, cover: null, preview: null });
  useEffect(
    () => () => {
      const held = heldBlobUrlsRef.current;
      for (const url of [held.final, held.cover, held.preview]) {
        if (url?.startsWith('blob:')) URL.revokeObjectURL(url);
      }
    },
    [],
  );

  // 「打开到 Canvas」按钮的目标页面：进入工作台即预热 ProjectPage 大 chunk，
  // 悬停按钮时再补一次，保证点击跳转不再等 chunk 解析。
  useEffect(() => {
    void loadVideoCanvasProjectPage().catch(() => undefined);
  }, []);

  const [previewEpoch, setPreviewEpoch] = useState(0);
  const storyboardVisibleTracked = useRef(false);
  const [actionAssetsReady, setActionAssetsReady] = useState(false);
  /** Bump so the agent session re-lists Cameo stills after home upload. */
  const [cameoRefreshToken, setCameoRefreshToken] = useState(0);
  const [focusSceneId, setFocusSceneId] = useState<string | null>(null);
  const studioShellRef = useRef<HTMLDivElement>(null);
  const [studioShellWidth, setStudioShellWidth] = useState(1280);
  const [sessionRatio, setSessionRatio] = useState(loadStudioSessionWidthRatio);
  const [sessionCollapsed, setSessionCollapsed] = useState(loadStudioSessionCollapsed);
  const sessionWidth = computeStudioSessionWidth(studioShellWidth, sessionRatio);

  useLayoutEffect(() => {
    const node = studioShellRef.current;
    if (!node) return;
    const apply = () => {
      const next = node.getBoundingClientRect().width;
      if (next > 0) setStudioShellWidth(next);
    };
    apply();
    const observer = new ResizeObserver(apply);
    observer.observe(node);
    return () => observer.disconnect();
  }, [sessionId, loading]);

  const handleSessionWidthChange = useCallback((next: number) => {
    setSessionRatio((prev) => {
      const ratio = studioShellWidth > 0 ? next / studioShellWidth : prev;
      saveStudioSessionWidthRatio(ratio);
      return ratio;
    });
  }, [studioShellWidth]);

  const handleSessionCollapsedChange = useCallback((next: boolean) => {
    setSessionCollapsed(next);
    saveStudioSessionCollapsed(next);
  }, []);

  const launchState = (location.state as WorkspaceLaunchState | null) ?? null;
  const launchDraft = launchState?.launchDraft;
  const shouldAutoPlan = Boolean(launchState?.autoPlan) && !launchState?.launchError;
  const sourceDocumentName =
    launchDraft?.sourceDocumentName?.trim() || readStoredSourceDocument(sessionId);

  useEffect(() => {
    const name = launchDraft?.sourceDocumentName?.trim();
    if (name) storeSourceDocument(sessionId, name);
  }, [sessionId, launchDraft?.sourceDocumentName]);

  const sourceField = session ? sourceFieldForWorkflow(session.workflow) : 'idea';

  const sourcePlaceholder = useMemo(() => {
    switch (sourceField) {
      case 'script':
        return t('videoGeneration.workspace.source.scriptPlaceholder', {
          defaultValue:
            '粘贴完整剧本…默认拍全集；需求可写「拍第N集」「前N场」缩小范围',
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
      setStyle(s.style?.trim() || launchDraft?.style?.trim() || '');
      setAspectRatio(
        normalizeSeedanceAspectRatio(
          s.aspect_ratio ||
            launchDraft?.preferences.aspectRatio ||
            DEFAULT_SEEDANCE_ASPECT_RATIO
        )
      );
      const videoModel = s.video_model || launchDraft?.preferences.models.video_model || '';
      setModels({
        llm_model: s.llm_model || launchDraft?.preferences.models.llm_model || '',
        image_model: s.image_model || launchDraft?.preferences.models.image_model || '',
        video_model: videoModel,
      });
      setResolution(
        normalizeVideoResolution(
          videoModel,
          s.resolution ||
            launchDraft?.preferences.resolution ||
            DEFAULT_VIDEO_RESOLUTION
        )
      );
      setFps(
        normalizeVideoFps(
          videoModel,
          typeof s.fps === 'number' && s.fps > 0
            ? s.fps
            : launchDraft?.preferences.fps ?? DEFAULT_VIDEO_FPS
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

  // When the video model changes, remap Seedance `720p` → MiniMax-H3 `768P` (etc.).
  useEffect(() => {
    if (!models.video_model.trim()) return;
    const nextRes = normalizeVideoResolution(models.video_model, resolution);
    const nextFps = normalizeVideoFps(models.video_model, fps);
    if (nextRes !== resolution) setResolution(nextRes);
    if (nextFps !== fps) setFps(nextFps);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- clamp only on model change
  }, [models.video_model]);

  const refreshArtifacts = useCallback(async () => {
    if (!sessionId) return;
    try {
      setArtifacts(await listArtifacts(sessionId));
    } catch (e) {
      console.warn('[videoGeneration] artifacts refresh failed', e);
    }
  }, [sessionId]);

  // Coarse run-status signals. Heartbeat ticks that only advance stage /
  // message keep flag identity stable so the heavy page body skips re-render;
  // live-progress components subscribe to the full snapshot themselves
  // (see ./useRunStatusFeed).
  const statusFlags = useRunStatusFlags();
  const prevRunStatusRef = useRef<string | null>(null);
  const creditsFailToastKeyRef = useRef<string | null>(null);
  const lastArtifactRefreshKeyRef = useRef<string | null>(null);
  const lastPeriodicArtifactAtRef = useRef(0);

  // Mounts the self-scheduling status poll (1s active-visible / 5s otherwise,
  // no request pile-up). Per-poll side effects stay here: merge coarse fields
  // into the session record without identity churn, and rescan artifacts when
  // clips land. Artifact scans pause entirely while the tab is hidden.
  const refreshRun = useRunStatusFeedController(sessionId, (st) => {
    setSession((prev) => {
      if (!prev) return prev;
      const final_video = st.final_video ?? prev.final_video;
      const cover = st.cover ?? prev.cover;
      const credits_consumed = Math.max(
        Number(prev.credits_consumed ?? 0) || 0,
        Number(st.credits_consumed ?? 0) || 0
      );
      if (
        prev.status === st.status &&
        prev.stage === st.stage &&
        prev.final_video === final_video &&
        prev.cover === cover &&
        (Number(prev.credits_consumed ?? 0) || 0) === credits_consumed
      ) {
        return prev;
      }
      return {
        ...prev,
        status: st.status,
        stage: st.stage,
        final_video,
        cover,
        credits_consumed,
      };
    });
    if (document.hidden) return;

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
      'design_storyboard',
      'decompose_shots',
      'construct_camera_tree',
      'planned',
      'reuse_plan',
    ]);
    // Include message + updated_at so consecutive shots finishing with the
    // same stage name still trigger a refresh.
    const refreshKey = `${stage}:${st.message ?? ''}:${st.updated_at ?? ''}`;
    if (artifactLandingStages.has(stage) && refreshKey !== lastArtifactRefreshKeyRef.current) {
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
  });

  useEffect(() => {
    void loadSession();
  }, [loadSession]);

  useEffect(() => {
    if (!sessionId || loading || loadError) return;
    void refreshArtifacts();
    void refreshRun();
  }, [sessionId, loading, loadError, refreshArtifacts, refreshRun]);

  // Reset transition tracking when switching sessions.
  useEffect(() => {
    prevRunStatusRef.current = null;
    creditsFailToastKeyRef.current = null;
  }, [sessionId]);

  // Toast only on an active→failed transition so revisiting an old failed job
  // does not re-spam; the agent session still shows the credits failure either way.
  useEffect(() => {
    const prev = prevRunStatusRef.current;
    const next = statusFlags.status;
    prevRunStatusRef.current = next;
    if (sessionId && next === 'succeeded' && statusFlags.hasFinalVideo) {
      trackVideoSessionEvent('film_succeeded', sessionId, {
        workflow: session?.workflow ?? null,
        status: next,
      });
      if (hasVideoSessionEvent('resume_started', sessionId)) {
        trackVideoSessionEvent('resume_succeeded', sessionId, {
          workflow: session?.workflow ?? null,
        });
      }
    }
    if (
      sessionId &&
      (next === 'failed' || next === 'cancelled' || next === 'interrupted') &&
      (prev === 'planning' || prev === 'rendering')
    ) {
      const snapshot = getRunStatusSnapshot();
      const stage = snapshot?.stage ?? '';
      const errText = snapshot?.error ?? '';
      const failureChannel = /plan|brief|script|storyboard/i.test(stage)
        ? 'llm'
        : /image|poster|cover/i.test(stage)
          ? 'image'
          : /video|render|concat/i.test(stage)
            ? 'video'
            : 'pipeline';
      if (next === 'cancelled' || next === 'interrupted') {
        trackVideoSessionEvent('film_cancelled', sessionId, {
          workflow: session?.workflow ?? null,
          status: next,
        });
      } else {
        const { errorCode, errorMessage } = filmTelemetryError(errText);
        trackVideoSessionEvent('film_failed', sessionId, {
          workflow: session?.workflow ?? null,
          status: next,
          failure_channel: errorCode?.includes('.') ? 'video' : failureChannel,
          error_code: statusFlags.creditsFailed
            ? 'insufficient_credits'
            : errorCode,
          error_message: errorMessage,
        });
      }
    }
    if (!sessionId || next !== 'failed' || !statusFlags.creditsFailed) return;
    if (prev !== 'planning' && prev !== 'rendering') return;
    const key = `${sessionId}:credits`;
    if (creditsFailToastKeyRef.current === key) return;
    creditsFailToastKeyRef.current = key;
    message.error(
      t('videoGeneration.workspace.failure.creditsToast', {
        defaultValue: '积分不足，请充值或缩短时长后从断点继续。',
      })
    );
  }, [
    session?.workflow,
    sessionId,
    statusFlags.creditsFailed,
    statusFlags.hasFinalVideo,
    statusFlags.status,
    message,
    t,
  ]);

  // Load artifact preview when selection changes (blob URLs for media + auth).
  useEffect(() => {
    if (!sessionId || !selectedPath) {
      setPreview((prev) => {
        if (prev?.url?.startsWith('blob:')) URL.revokeObjectURL(prev.url);
        return null;
      });
      heldBlobUrlsRef.current.preview = null;
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
        heldBlobUrlsRef.current.preview = content.url ?? null;
      })
      .catch((e) => {
        if (!cancelled) {
          setPreview((prev) => {
            if (prev?.url?.startsWith('blob:')) URL.revokeObjectURL(prev.url);
            heldBlobUrlsRef.current.preview = null;
            return {
              kind: 'text',
              text: e instanceof Error ? e.message : String(e),
            };
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
    const rel = statusFlags.finalVideoPath || session?.final_video;
    if (!sessionId || !rel) {
      setFinalBlobUrl((prev) => {
        if (prev?.startsWith('blob:')) URL.revokeObjectURL(prev);
        return null;
      });
      heldBlobUrlsRef.current.final = null;
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
        heldBlobUrlsRef.current.final = url;
      })
      .catch((e) => {
        console.warn('[videoGeneration] final video load failed', e);
      });
    return () => {
      cancelled = true;
    };
  }, [sessionId, statusFlags.finalVideoPath, session?.final_video]);

  // Film poster (display-only) via authenticated blob URL.
  useEffect(() => {
    const rel = statusFlags.coverPath || session?.cover;
    if (!sessionId || !rel) {
      setCoverBlobUrl((prev) => {
        if (prev?.startsWith('blob:')) URL.revokeObjectURL(prev);
        return null;
      });
      heldBlobUrlsRef.current.cover = null;
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
        heldBlobUrlsRef.current.cover = url;
      })
      .catch((e) => {
        console.warn('[videoGeneration] cover load failed', e);
      });
    return () => {
      cancelled = true;
    };
  }, [sessionId, statusFlags.coverPath, session?.cover]);

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
    patchRunStatus(
      getRunStatusSnapshot()
        ? { status: 'planning' }
        : { stage: 'planning', message: '', progress: 0, status: 'planning' }
    );
    try {
      const fromSession =
        session.vertical_skill_ids && session.vertical_skill_ids.length > 0
          ? session.vertical_skill_ids
          : undefined;
      const fromLaunch =
        launchDraft?.verticalSkillIds && launchDraft.verticalSkillIds.length > 0
          ? launchDraft.verticalSkillIds
          : undefined;
      const prefs = launchDraft?.preferences;
      const body = {
        [sourceField]: trimmed,
        user_requirement: requirement.trim() || undefined,
        style: style.trim() || undefined,
        vertical_skill_ids: fromSession ?? fromLaunch,
        aspect_ratio: aspectRatio,
        resolution: normalizeVideoResolution(models.video_model, resolution),
        fps: normalizeVideoFps(models.video_model, fps),
        llm_model: models.llm_model.trim() || undefined,
        image_model: models.image_model.trim() || undefined,
        video_model: models.video_model.trim() || undefined,
        target_duration_secs:
          prefs?.mediaKind === 'video' && prefs.specifyTargetDuration
            ? clampDuration(prefs.targetDurationSecs)
            : undefined,
      };
      await planSession(sessionId, body);
      message.success(t('videoGeneration.workspace.planStarted', { defaultValue: '已开始规划' }));
      setCameoRefreshToken((n) => n + 1);
      const st = await refreshRun();
      if (!st || !isActiveStatus(st.status)) {
        // Optimistic: mark planning so polling kicks in even if status lags.
        patchRunStatus(
          getRunStatusSnapshot()
            ? { status: 'planning' }
            : { stage: 'planning', message: '', progress: 0, status: 'planning' }
        );
      }
      void refreshArtifacts();
    } catch (e) {
      const raw = e instanceof Error ? e.message : String(e);
      // Home auto-plan can race Strict Mode; ignore "already running".
      if (/already has an active job/i.test(raw)) {
        void refreshRun();
        return;
      }
      void refreshRun();
      message.error(
        isInsufficientCreditsError(raw)
          ? t('videoGeneration.workspace.failure.creditsToast', {
              defaultValue: '积分不足，请充值或缩短时长后从断点继续。',
            })
          : `${t('videoGeneration.workspace.planFailed', { defaultValue: '规划失败' })}: ${raw}`
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
    aspectRatio,
    resolution,
    fps,
    models,
    launchDraft,
    message,
    t,
    refreshRun,
    refreshArtifacts,
  ]);

  // Home「发送」lands here with autoPlan: sync any in-memory refs, then plan.
  useEffect(() => {
    if (!shouldAutoPlan || !sessionId || loading || loadError || !session) return;
    if (autoPlannedSessions.has(sessionId)) return;
    if (isActionImitationWorkflow(session.workflow)) return;
    const clearAutoPlanFlag = () => {
      navigate(`${location.pathname}${location.search}`, {
        replace: true,
        // Keep launchDraft so brief fields stay filled; drop autoPlan to avoid loops.
        state: launchDraft ? { launchDraft } : {},
      });
    };
    if (
      isActiveStatus(session.status) ||
      session.stage === 'planned' ||
      session.status === 'succeeded'
    ) {
      autoPlannedSessions.add(sessionId);
      clearAutoPlanFlag();
      return;
    }
    autoPlannedSessions.add(sessionId);
    clearAutoPlanFlag();
    void (async () => {
      // Home may have shown thumbnails while HTTP upload failed (e.g. old 11MB
      // body limit). Retry from in-memory Files before planning locks inputs.
      const pending = (launchDraft?.cameos ?? []).filter((c) => c.file);
      if (pending.length > 0) {
        try {
          const existing = await listCameos(sessionId);
          if (existing.length === 0) {
            for (const [index, cameo] of pending.entries()) {
              await uploadCameo(
                sessionId,
                cameo.file!,
                cameo.characterName.trim() || `参考图${index + 1}`,
                cameo.description.trim()
              );
            }
            setCameoRefreshToken((n) => n + 1);
          }
        } catch (e) {
          message.error(
            `${t('videoGeneration.workspace.uploadFailed', {
              defaultValue: '上传参考图失败',
            })}: ${e instanceof Error ? e.message : String(e)}`
          );
        }
      }
      await handlePlan();
    })();
  }, [
    shouldAutoPlan,
    sessionId,
    loading,
    loadError,
    session,
    handlePlan,
    navigate,
    location.pathname,
    location.search,
    launchDraft,
    message,
    t,
  ]);

  const handleSaveSceneDescriptions = useCallback(
    async (scene: StoryboardScene, descriptions: StoryboardSceneSave) => {
      if (!sessionId) return;
      const targetPath =
        scene.storyboardPath ||
        (scene.sceneRoot
          ? `${scene.sceneRoot.replace(/\\/g, '/')}/storyboard.json`
          : '') ||
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
        const patched = patchShotDescriptionsInArtifact(current.text, scene, {
          visualDescription: descriptions.visualDescription ?? '',
          audioDescription: descriptions.audioDescription,
        });
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
    const actionMode = isActionImitationWorkflow(session?.workflow);
    if (actionMode) {
      if (!models.video_model.trim()) {
        message.warning(
          t('videoGeneration.workspace.models.videoRequired', {
            defaultValue: '请先选择视频模型',
          })
        );
        return;
      }
    } else if (!models.image_model.trim() || !models.video_model.trim()) {
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
        llm_model: actionMode ? undefined : models.llm_model.trim() || undefined,
        image_model: actionMode ? undefined : models.image_model.trim() || undefined,
        video_model: models.video_model.trim() || undefined,
        resolution: normalizeVideoResolution(models.video_model, resolution),
        fps: normalizeVideoFps(models.video_model, fps),
      });
      trackFunnelEvent('render_started', {
        feature: 'video_generation',
        session_id: sessionId,
        workflow: session?.workflow ?? null,
        source: statusFlags.failedLike ? 'resume' : 'workspace',
      });
      message.success(t('videoGeneration.workspace.renderStarted', { defaultValue: '已开始渲染' }));
      const st = await refreshRun();
      if (!st || !isActiveStatus(st.status)) {
        patchRunStatus(
          getRunStatusSnapshot()
            ? { status: 'rendering' }
            : { stage: 'render', message: '', progress: 0, status: 'rendering' }
        );
      }
    } catch (e) {
      const raw = e instanceof Error ? e.message : String(e);
      message.error(
        isInsufficientCreditsError(raw)
          ? t('videoGeneration.workspace.failure.creditsToast', {
              defaultValue: '积分不足，请充值或缩短时长后从断点继续。',
            })
          : `${t('videoGeneration.workspace.renderFailed', { defaultValue: '渲染失败' })}: ${raw}`
      );
    } finally {
      setRendering(false);
    }
  }, [
    sessionId,
    session?.workflow,
    models,
    resolution,
    fps,
    statusFlags.failedLike,
    message,
    t,
    refreshRun,
  ]);

  const handleCancel = useCallback(async () => {
    if (!sessionId) return;
    setCancelling(true);
    try {
      await cancelSession(sessionId);
      message.info(t('videoGeneration.workspace.cancelOk', { defaultValue: '已请求取消' }));
      await refreshRun();
    } catch (e) {
      message.error(
        `${t('videoGeneration.workspace.cancelFailed', { defaultValue: '取消失败' })}: ${
          e instanceof Error ? e.message : String(e)
        }`
      );
    } finally {
      setCancelling(false);
    }
  }, [sessionId, message, t, refreshRun]);

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

  const handleRename = useCallback(
    async (next: string) => {
      if (!sessionId) return;
      try {
        const updated = await updateSessionTitle(sessionId, next);
        setSession((prev) => (prev ? { ...prev, title: updated.title } : updated));
        updateRecentVideoGenerationTitle(sessionId, updated.title);
      } catch (e) {
        message.error(
          `${t('videoGeneration.workspace.renameFailed', { defaultValue: '标题保存失败' })}: ${
            e instanceof Error ? e.message : String(e)
          }`
        );
      }
    },
    [sessionId, message, t]
  );

  // Subscribe to the full snapshot only while a terminal failure is showing —
  // resume-vs-replan is the sole page-level consumer of raw events.
  const failedRunStatus = useRunStatusFull(statusFlags.failedLike);

  /** Prefer resume render when failure/cancel happened in a render-phase stage. */
  const continueAsRender = useMemo(
    () =>
      shouldContinueAsRender({
        events: failedRunStatus?.events,
        stage: failedRunStatus?.stage,
        sessionStage: session?.stage,
      }),
    [failedRunStatus, session?.stage]
  );

  const liveStatus = statusFlags.status ?? session?.status ?? null;
  const isFailed =
    statusFlags.failedLike ||
    (statusFlags.status == null &&
      (session?.status === 'failed' ||
        session?.status === 'cancelled' ||
        session?.status === 'interrupted'));

  const handleContinue = useCallback(() => {
    if (sessionId) {
      trackVideoSessionEvent('resume_started', sessionId, {
        workflow: session?.workflow ?? null,
        phase:
          isActionImitationWorkflow(session?.workflow) || continueAsRender
            ? 'render'
            : 'plan',
      });
    }
    if (isActionImitationWorkflow(session?.workflow) || continueAsRender) {
      void handleRender();
    } else {
      void handlePlan();
    }
  }, [continueAsRender, handleRender, handlePlan, session?.workflow, sessionId]);

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
    if (statusFlags.busy || planning || rendering) {
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
      trackVideoSessionEvent('project_exported', sessionId, {
        workflow: session?.workflow ?? null,
      });
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
    statusFlags.busy,
    session?.title,
    session?.workflow,
    sessionId,
    t,
  ]);

  const handlePublishToTvShow = useCallback(async () => {
    if (publishing || !sessionId || !session) return;
    if (cloudStatus !== 'authenticated') {
      message.warning(
        t('videoGeneration.tvShow.authRequired.publish', {
          defaultValue: '发布到 Flowy TV 需要先登录云端账号。',
        })
      );
      navigate('/cloud-login');
      return;
    }
    const status = statusFlags.status ?? session.status;
    const hasFilm = Boolean(statusFlags.finalVideoPath || session.final_video);
    const hasCover = Boolean(statusFlags.coverPath || session.cover);
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
    if (statusFlags.busy || planning || rendering) {
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
      trackVideoSessionEvent('tv_published', sessionId, {
        workflow: session.workflow,
      });
      message.success(
        t('videoGeneration.tvShow.publish.ok', {
          defaultValue: '已提交审核，通过后会出现在 Flowy TV 广场。',
        })
      );
    } catch (e) {
      if (isInvalidCloudSessionError(e)) {
        await logout();
        navigate('/cloud-login');
        return;
      }
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
    logout,
    message,
    navigate,
    planning,
    publishing,
    rendering,
    statusFlags.busy,
    statusFlags.coverPath,
    statusFlags.finalVideoPath,
    statusFlags.status,
    session,
    sessionId,
    t,
  ]);

  const handleRevealFilm = useCallback(async () => {
    // Click-time snapshot read — no reactive subscription needed for a one-shot action.
    const snap = getRunStatusSnapshot();
    const rel = ((snap?.final_video || session?.final_video || '') as string)
      .replace(/\\/g, '/')
      .replace(/^\/+/, '');
    const root = ((snap?.working_dir_abs || '') as string)
      .replace(/\\/g, '/')
      .replace(/\/+$/, '');
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
  }, [message, session?.final_video, sessionId, t]);

  const busy = statusFlags.busy || planning || rendering;
  const isAction = isActionImitationWorkflow(session?.workflow);
  const videoCreditsConsumed = resolveSessionCreditsConsumed({
    sessionCredits: session?.credits_consumed,
    statusCredits: statusFlags.creditsConsumed,
  });
  const hasStoryboard =
    !isAction &&
    (Boolean(findStoryboardPath(artifacts)) ||
      session?.stage === 'planned' ||
      statusFlags.stagePlanned ||
      statusFlags.status === 'rendering' ||
      statusFlags.status === 'succeeded' ||
      session?.status === 'succeeded');
  const canRender = isAction
    ? !busy && actionAssetsReady
    : !busy && (hasStoryboard || isFailed);
  /** Resume is only needed when continuing as plan (render button handles resume-as-render). */
  const currentStatus = liveStatus;
  const canPublishTvShow =
    !busy &&
    !publishing &&
    currentStatus === 'succeeded' &&
    (statusFlags.hasFinalVideo || Boolean(session?.final_video)) &&
    (statusFlags.hasCover || Boolean(session?.cover));
  const canOpenInCanvas =
    !isAction &&
    !busy &&
    !materializing &&
    (hasStoryboard ||
      statusFlags.hasFinalVideo ||
      Boolean(session?.final_video) ||
      currentStatus === 'succeeded');

  const handleOpenInCanvas = useCallback(async () => {
    if (!sessionId || materializing || busy) return;
    setMaterializing(true);
    try {
      const result = await materializeSessionToCanvas(sessionId);
      if (result.reused) {
        message.success(
          t('videoGeneration.actions.openInCanvasReused', {
            defaultValue: '已打开该工程对应的 Canvas（未新建）',
          })
        );
      } else {
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
      }
      navigate(videoCanvasProjectPath(result.project_id));
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
      ref={studioShellRef}
      className={[
        styles.studioPage,
        styles.studioShell,
        'flex-1 min-h-0 size-full box-border',
      ].join(' ')}
    >
      {messageHolder}
      <header className={styles.studioHeader}>
        <div className='flex items-center justify-between gap-12px flex-wrap'>
          <div className='flex items-center gap-8px min-w-0'>
            <CanvasChromeButton
              className='is-icon shrink-0'
              onClick={() => navigate('/video-generation')}
              title={t('videoGeneration.workspace.back', { defaultValue: '返回列表' })}
              aria-label={t('videoGeneration.workspace.back', { defaultValue: '返回列表' })}
            >
              <ArrowLeft theme='outline' size={16} fill='currentColor' />
            </CanvasChromeButton>
            <div className='min-w-0'>
              <WorkspaceTitleField title={session.title} onSave={handleRename} />
              <p className='m-0 mt-2px text-11px text-[var(--color-text-3)] truncate'>
                {workflowLabel(session.workflow, t)} · {statusLabel(currentStatus, t)}
              </p>
            </div>
          </div>
          <div className='flex items-center gap-4px shrink-0'>
            {sessionCollapsed && !isMobile ? (
              <CanvasChromeButton
                className='is-icon'
                onClick={() => handleSessionCollapsedChange(false)}
                title={t('videoGeneration.agentSession.expand', { defaultValue: '展开会话' })}
                aria-label={t('videoGeneration.agentSession.expand', { defaultValue: '展开会话' })}
              >
                <Robot theme='outline' size={14} fill='currentColor' />
              </CanvasChromeButton>
            ) : null}
            <CanvasChromeButton
              className='is-icon'
              onClick={() => {
                void refreshRun();
                void refreshArtifacts();
              }}
              title={t('videoGeneration.workspace.refresh', { defaultValue: '刷新' })}
              aria-label={t('videoGeneration.workspace.refresh', { defaultValue: '刷新' })}
            >
              <Refresh theme='outline' size={14} fill='currentColor' />
            </CanvasChromeButton>
            <CanvasChromeButton
              className='is-icon'
              disabled={materializing || !canOpenInCanvas}
              title={
                isAction
                  ? t('videoGeneration.actions.openInCanvasUnsupported', {
                      defaultValue: '动作模仿没有分镜，无法打开到 Canvas',
                    })
                  : t('videoGeneration.actions.openInCanvas', { defaultValue: '打开到 Canvas' })
              }
              aria-label={t('videoGeneration.actions.openInCanvas', { defaultValue: '打开到 Canvas' })}
              onPointerEnter={() => void loadVideoCanvasProjectPage().catch(() => undefined)}
              onClick={() => void handleOpenInCanvas()}
            >
              <Cube theme='outline' size={14} fill='currentColor' />
            </CanvasChromeButton>
            <CanvasChromeButton
              className='is-icon'
              disabled={busy || exporting}
              title={t('videoGeneration.actions.exportProject', { defaultValue: '导出工程' })}
              aria-label={t('videoGeneration.actions.exportProject', { defaultValue: '导出工程' })}
              onClick={() => void handleExportProject()}
            >
              <Export theme='outline' size={14} fill='currentColor' />
            </CanvasChromeButton>
            <CanvasChromeButton
              className='is-icon'
              disabled={!canPublishTvShow || publishing}
              title={t('videoGeneration.tvShow.publish.action', { defaultValue: '发布到 Flowy TV' })}
              aria-label={t('videoGeneration.tvShow.publish.action', { defaultValue: '发布到 Flowy TV' })}
              onClick={() => void handlePublishToTvShow()}
            >
              <Share theme='outline' size={14} fill='currentColor' />
            </CanvasChromeButton>
            <Popconfirm
              title={t('videoGeneration.actions.deleteConfirm', {
                defaultValue: '确定删除该任务？产物将一并清除。',
              })}
              disabled={deleting}
              onOk={() => void handleDelete()}
            >
              <CanvasChromeButton
                className='is-icon'
                disabled={deleting}
                title={t('videoGeneration.actions.delete', { defaultValue: '删除' })}
                aria-label={t('videoGeneration.actions.delete', { defaultValue: '删除' })}
              >
                <Delete theme='outline' size={14} fill='currentColor' />
              </CanvasChromeButton>
            </Popconfirm>
          </div>
        </div>
      </header>

      <div className={styles.studioBody}>
        <div className={styles.studioMain}>
          <div className={styles.studioMainInner}>
        {isAction ? (
          <section className={`${styles.studioPanel} p-16px md:p-20px`}>
            <div className='mb-14px'>
              <h2 className='m-0 text-16px font-650 text-[var(--color-text-1)]'>
                {t('videoGeneration.studio.actionTitle', { defaultValue: '上传素材，生成成片' })}
              </h2>
              <p className='m-0 mt-3px text-12px text-[var(--color-text-3)]'>
                {t('videoGeneration.studio.actionHint', {
                  defaultValue: '一张角色图 + 一段参考视频。无需提示词，时长跟随参考视频。',
                })}
              </p>
            </div>
            <WorkspaceActionAssets
              sessionId={sessionId}
              disabled={busy}
              onReadyChange={setActionAssetsReady}
            />
            <div className='mt-14px'>
              <ModelSelectors
                value={models}
                onChange={setModels}
                disabled={busy}
                isMobile={isMobile}
                mode='action'
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
          </section>
        ) : !hasStoryboard ? (
          <section className={`${styles.studioPanel} p-16px md:p-20px`}>
            <div className='mb-14px'>
              <h2 className='m-0 text-16px font-650 text-[var(--color-text-1)]'>
                {t('videoGeneration.studio.briefTitle', { defaultValue: '把故事交给 Flowy' })}
              </h2>
              <p className='m-0 mt-3px text-12px text-[var(--color-text-3)]'>
                {t('videoGeneration.studio.briefHint', {
                  defaultValue: '生成的是可修改分镜，不会直接开始高成本渲染。',
                })}
              </p>
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
            <div className={`mt-12px grid gap-10px ${isMobile ? 'grid-cols-1' : 'grid-cols-2'}`}>
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
              <div className='flex flex-col gap-6px text-12px text-[var(--color-text-3)]'>
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
        ) : null}

        {finalBlobUrl ? (
          <section className={`${styles.studioPanel} overflow-hidden`}>
            <div className='flex flex-wrap items-center justify-between gap-10px px-16px py-13px'>
              <div>
                <div className='flex flex-wrap items-center gap-7px text-14px font-650 text-[var(--color-text-1)]'>
                  <VideoOne
                    theme='outline'
                    size={16}
                    className='text-[rgb(var(--primary-6))]'
                  />
                  {t('videoGeneration.studio.filmReady', { defaultValue: '成片已就绪' })}
                  {videoCreditsConsumed > 0 ? (
                    <span
                      data-testid='session-video-credits'
                      className='text-12px font-500 tabular-nums text-[var(--color-text-3)]'
                    >
                      {t('videoGeneration.studio.creditsConsumed', {
                        credits: videoCreditsConsumed,
                        defaultValue: '消耗 {{credits}} 积分',
                      })}
                    </span>
                  ) : null}
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

        {!isAction && hasStoryboard ? (
          <section className={`${styles.studioPanel} ${styles.storyboardPanel}`}>
            <div className={styles.storyboardHeader}>
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
              revising={revising}
              focusSceneId={focusSceneId}
              onSaveSceneDescriptions={handleSaveSceneDescriptions}
            />
          </section>
        ) : null}

        {!isAction && hasStoryboard ? (
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
        ) : null}

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
                      defaultValue:
                        '审阅参考图分类、定妆图、环境/道具板与工程文件，再生成成片。',
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
          </div>
        </div>
        {isMobile || !sessionCollapsed ? (
          <StudioAgentSession
            sessionId={sessionId}
            artifacts={artifacts}
            sourceText={sourceText}
            hasStoryboard={hasStoryboard}
            hasFinalVideo={Boolean(finalBlobUrl) || statusFlags.hasFinalVideo}
            coverPath={statusFlags.coverPath || session.cover}
            finalVideoPath={statusFlags.finalVideoPath || session.final_video}
            isAction={isAction}
            actionAssetsReady={actionAssetsReady}
            canRender={canRender}
            isFailed={Boolean(isFailed)}
            busy={busy}
            planning={planning}
            rendering={rendering}
            cancelling={cancelling}
            creditsConsumed={videoCreditsConsumed}
            models={models}
            isMobile={isMobile}
            collapsed={sessionCollapsed}
            width={sessionWidth}
            onCollapsedChange={handleSessionCollapsedChange}
            onWidthChange={handleSessionWidthChange}
            onPlan={() => void handlePlan()}
            onRender={() => void handleRender()}
            onCancel={() => void handleCancel()}
            onContinue={handleContinue}
            onFocusScene={setFocusSceneId}
            onSelectArtifact={setSelectedPath}
            cameoEpoch={cameoRefreshToken}
            sourceDocumentName={sourceDocumentName}
          />
        ) : null}
      </div>
    </div>
  );
};

export default WorkspacePage;
