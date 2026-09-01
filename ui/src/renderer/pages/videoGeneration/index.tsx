
/**
 * Unified video creation home (`/video-generation`).
 * Agent and infinite-canvas creation share one composer while keeping their
 * skills, drafts, submissions, and project galleries independent.
 */
import React, {
  Suspense,
  lazy,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Button, Result, Spin } from '@arco-design/web-react';
import { Search, Upload, VideoOne } from '@icon-park/react';
import SegmentedTabs, { type SegmentedTabItem } from '@renderer/components/base/SegmentedTabs';
import { useLayoutContext } from '@renderer/hooks/context/LayoutContext';
import { useArcoMessage } from '@renderer/utils/ui/useArcoMessage';
import { isDesktopShell } from '@renderer/utils/platform';
import { trackFunnelEvent } from '@renderer/utils/analytics/productFunnel';
import {
  createSession,
  deleteSession,
  importSession,
  listSessions,
  renderSession,
  uploadActionAssets,
  uploadCameo,
} from './api';
import { isInvalidCloudSessionError } from '@/common/adapter/httpBridge';
import { useCloudAuth } from '@renderer/hooks/context/CloudAuthContext';
import type { SessionSummary } from './types';
import VideoHomeComposer, { clearVideoHomeDraft } from './home/VideoHomeComposer';
import { prefetchCanvasWorkspace } from './prefetch';
import { videoCanvasProjectPath } from '../videoCanvas/routes';
import {
  briefingWorkspacePath,
  createBriefing,
  listBriefingSessions,
  runBriefing,
} from './briefing/api';
import type { BriefingSessionSummary } from './briefing/api';
import { parseVideoHomeMode, type VideoCreateDraft, type VideoHomeMode } from './home/types';
import {
  CLIP_DURATION_DEFAULT_SECS,
  CLIP_DURATION_MAX_SECS,
  CLIP_DURATION_MIN_SECS,
  CLIP_DURATION_STEP_SECS,
  clampDuration,
} from './durationBounds';
import {
  clearVideoGenerationSessionMemory,
  rememberVideoGenerationSession,
  rememberVideoGenerationTask,
} from './routeMemory';
import { isInsufficientCreditsError } from './creditsError';
import type { GenerationTaskView } from '../videoCanvas/api';
import styles from './index.module.css';

/**
 * Video generation mode ("视频生成") uses Canvas direct video generation API
 * and navigates to a dedicated clip result page instead of the agent workspace.
 */

/** Lazily load OC bridge so the list page chunk does not pull @oc/stores. */
async function createServerBackedCanvasProject(
  ...args: Parameters<
    typeof import('../videoCanvas/lib/ocBridge').createServerBackedCanvasProject
  >
) {
  const { createServerBackedCanvasProject: create } = await import(
    '../videoCanvas/lib/ocBridge'
  );
  return create(...args);
}

type ListTab = 'recent' | 'tvShow';

const CanvasProjectGallery = lazy(() => import('./home/CanvasProjectGallery'));
const TvShowPanel = lazy(() => import('./components/TvShowPanel'));
const SessionCard = lazy(() => import('./components/SessionCard'));
const GenerationTaskCard = lazy(() => import('./components/GenerationTaskCard'));

function titleForDraft(draft: VideoCreateDraft): string {
  if (draft.workflow === 'action2video') {
    const fromCharacter = draft.actionCharacter?.file.name.replace(/\.[^.]+$/, '').trim();
    return fromCharacter?.slice(0, 48) || '';
  }
  return draft.sourceText.split(/\r?\n/, 1)[0]?.trim().slice(0, 48) || '';
}

const VideoGenerationListPage: React.FC = () => {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [searchParams, setSearchParams] = useSearchParams();
  const layout = useLayoutContext();
  const isMobile = layout?.isMobile ?? false;
  const [message, messageHolder] = useArcoMessage();
  const { logout } = useCloudAuth();

  const workMode: VideoHomeMode = parseVideoHomeMode(searchParams.get('mode'));

  const [listTab, setListTab] = useState<ListTab>('tvShow');
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [briefingSessions, setBriefingSessions] = useState<BriefingSessionSummary[]>([]);
  const [generationTasks, setGenerationTasks] = useState<GenerationTaskView[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadingTasks, setLoadingTasks] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState('');
  const [creating, setCreating] = useState(false);
  const [importing, setImporting] = useState(false);
  const [deletingId, setDeletingId] = useState<string | null>(null);
  const pageScrollRef = useRef<HTMLDivElement>(null);
  const savedPageScrollTopRef = useRef(0);
  const initialWorkModeRef = useRef(workMode);

  useEffect(() => {
    trackFunnelEvent('home_viewed', {
      feature: 'video_generation',
      mode: initialWorkModeRef.current,
    });
  }, []);

  const handleListTabChange = useCallback((key: string) => {
    savedPageScrollTopRef.current = pageScrollRef.current?.scrollTop ?? 0;
    setListTab(key as ListTab);
  }, []);

  useLayoutEffect(() => {
    const page = pageScrollRef.current;
    if (!page) return;
    page.scrollTop = savedPageScrollTopRef.current;
  }, [listTab]);

  const listTabItems: SegmentedTabItem[] = useMemo(
    () => [
      {
        key: 'tvShow',
        label: t('videoGeneration.list.tabs.tvShow', { defaultValue: 'Flowy TV' }),
      },
      {
        key: 'recent',
        label: t('videoGeneration.list.tabs.recent', { defaultValue: '最近创作' }),
      },
    ],
    [t]
  );

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      setSessions(await listSessions());
      setError(null);
    } catch (e) {
      console.error('[videoGeneration] failed to load sessions', e);
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  const refreshBriefings = useCallback(async () => {
    setLoading(true);
    try {
      setBriefingSessions(await listBriefingSessions());
      setError(null);
    } catch (e) {
      console.error('[videoGeneration] failed to load briefing sessions', e);
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  const refreshTasks = useCallback(async () => {
    setLoadingTasks(true);
    try {
      const { listGenerationTasks } = await import('../videoCanvas/api');
      const result = await listGenerationTasks(30, 0);
      setGenerationTasks(result.tasks);
      setError(null);
    } catch (e) {
      console.error('[videoGeneration] failed to load generation tasks', e);
      // Don't show error for tasks - it's optional
      setGenerationTasks([]);
    } finally {
      setLoadingTasks(false);
    }
  }, []);

  useEffect(() => {
    if (workMode === 'briefing' && listTab === 'recent') {
      void refreshBriefings();
      return;
    }
    if (workMode !== 'creation' && listTab === 'recent') {
      void refresh();
    }
    if (workMode === 'generate' && listTab === 'recent') {
      void refreshTasks();
    }
  }, [listTab, refresh, refreshBriefings, refreshTasks, workMode]);

  // Keep generation-task polling going while the home is visible — recent
  // tasks need to keep ticking even when the user switches between
  // TvShow / recent tabs, and when generation is in-flight.
  useEffect(() => {
    if (workMode !== 'generate') return;
    void refreshTasks();
    const timer = window.setInterval(() => {
      void refreshTasks();
    }, 5000);
    return () => window.clearInterval(timer);
  }, [refreshTasks, workMode]);

  useEffect(() => {
    const prefetchGallery = () => {
      void import('./home/CanvasProjectGallery').catch(() => undefined);
    };
    const idleWindow = window as Window & {
      requestIdleCallback?: (callback: () => void, options?: { timeout?: number }) => number;
      cancelIdleCallback?: (handle: number) => void;
    };
    if (typeof idleWindow.requestIdleCallback === 'function') {
      const idleId = idleWindow.requestIdleCallback(prefetchGallery, { timeout: 1500 });
      return () => idleWindow.cancelIdleCallback?.(idleId);
    }
    const timer = window.setTimeout(prefetchGallery, 250);
    return () => window.clearTimeout(timer);
  }, []);

  // 「打开到 Canvas」/ 画布入口跳转到这里；用户在列表页停留时提前拉取并解析
  // ProjectPage 大 chunk，跳转时不再出现多秒骨架屏。
  useEffect(() => {
    prefetchCanvasWorkspace();
  }, []);

  const displayed = useMemo(() => {
    const q = searchQuery.trim().toLowerCase();
    if (!q) return sessions;
    return sessions.filter(
      (s) =>
        (s.title ?? '').toLowerCase().includes(q) ||
        s.workflow.toLowerCase().includes(q) ||
        (s.stage ?? '').toLowerCase().includes(q)
    );
  }, [sessions, searchQuery]);

  const displayedBriefings = useMemo(() => {
    const q = searchQuery.trim().toLowerCase();
    if (!q) return briefingSessions;
    return briefingSessions.filter(
      (session) =>
        session.title.toLowerCase().includes(q) ||
        session.stage.toLowerCase().includes(q) ||
        session.status.toLowerCase().includes(q)
    );
  }, [briefingSessions, searchQuery]);

  const displayedTasks = useMemo(() => {
    const q = searchQuery.trim().toLowerCase();
    if (!q) return generationTasks;
    return generationTasks.filter(
      (t) =>
        (t.prompt ?? '').toLowerCase().includes(q) ||
        t.task_id.toLowerCase().includes(q)
    );
  }, [generationTasks, searchQuery]);

  const handleCreate = useCallback(
    async (draft: VideoCreateDraft) => {
      if (creating) return;
      setCreating(true);
      try {
        const created = await createSession({
          workflow: draft.workflow,
          title: titleForDraft(draft) || undefined,
        });
        trackFunnelEvent('task_accepted', {
          feature: 'video_generation',
          workflow: draft.workflow,
          session_id: created.id,
        });
        try {
          if (draft.workflow === 'action2video') {
            if (!draft.actionCharacter?.file || !draft.actionVideo?.file) {
              throw new Error(
                t('videoGeneration.create.action.required', {
                  defaultValue: '请上传一张角色图和一个参考视频。',
                })
              );
            }
            await uploadActionAssets(created.id, {
              character: draft.actionCharacter.file,
              video: draft.actionVideo.file,
            });
            await renderSession(created.id, {
              video_model: draft.preferences.models.video_model || undefined,
              resolution: draft.preferences.resolution,
              fps: draft.preferences.fps,
            });
            trackFunnelEvent('render_started', {
              feature: 'video_generation',
              workflow: draft.workflow,
              session_id: created.id,
              source: 'action_create',
            });
          } else {
            // Upload refs here; planning starts on the workspace so the detail
            // page always enters the planning UI via the same path as「生成分镜」.
            const pendingCameos = draft.cameos.filter((c) => c.file);
            if (draft.cameos.length > 0 && pendingCameos.length === 0) {
              throw new Error(
                t('videoGeneration.create.upload.cameoFileMissing', {
                  defaultValue:
                    '参考图文件已失效（刷新页面后需重新选择图片），请重新添加后再发送。',
                })
              );
            }
            for (const [index, cameo] of pendingCameos.entries()) {
              await uploadCameo(
                created.id,
                cameo.file!,
                cameo.characterName.trim() || `参考图${index + 1}`,
                cameo.description.trim()
              );
            }
          }
          trackFunnelEvent('first_task_started', {
            feature: 'video_generation',
            workflow: draft.workflow,
            session_id: created.id,
          });
          clearVideoHomeDraft();
        } catch (launchError) {
          const raw = launchError instanceof Error ? launchError.message : String(launchError);
          if (draft.workflow === 'action2video') {
            trackFunnelEvent('film_failed', {
              feature: 'video_generation',
              workflow: draft.workflow,
              session_id: created.id,
              failure_channel: 'video',
              error_code: isInsufficientCreditsError(raw)
                ? 'insufficient_credits'
                : 'launch_failed',
            });
          }
          const failedLabel =
            draft.workflow === 'action2video'
              ? t('videoGeneration.workspace.renderFailed', { defaultValue: '渲染失败' })
              : t('videoGeneration.workspace.uploadFailed', {
                  defaultValue: '上传参考图失败',
                });
          message.error(
            isInsufficientCreditsError(raw)
              ? t('videoGeneration.workspace.failure.creditsToast', {
                  defaultValue: '积分不足，请充值或缩短时长后从断点继续。',
                })
              : `${failedLabel}: ${raw}`
          );
          navigate(`/video-generation/${created.id}`, {
            state: { launchDraft: draft, launchError: true },
          });
          rememberVideoGenerationSession(created.id, titleForDraft(draft));
          return;
        }
        rememberVideoGenerationSession(created.id, titleForDraft(draft));
        navigate(`/video-generation/${created.id}`, {
          state:
            draft.workflow === 'action2video'
              ? undefined
              : { launchDraft: draft, autoPlan: true },
        });
      } catch (e) {
        if (isInvalidCloudSessionError(e)) {
          await logout();
          navigate('/cloud-login');
          return;
        }
        message.error(
          `${t('videoGeneration.actions.createFailed', { defaultValue: '创建失败' })}: ${
            e instanceof Error ? e.message : String(e)
          }`
        );
      } finally {
        setCreating(false);
      }
    },
    [creating, logout, navigate, message, t]
  );

  const handleModeChange = useCallback(
    (mode: VideoHomeMode) => {
      const next = new URLSearchParams(searchParams);
      if (mode === 'creation') next.set('mode', 'creation');
      else if (mode === 'action') next.set('mode', 'action');
      else if (mode === 'generate') next.set('mode', 'generate');
      else if (mode === 'briefing') next.set('mode', 'briefing');
      else next.delete('mode');
      setSearchParams(next, { replace: true });
    },
    [searchParams, setSearchParams]
  );

  const handleCreateCanvas = useCallback(
    async (draft: VideoCreateDraft) => {
      if (creating) return;
      setCreating(true);
      prefetchCanvasWorkspace();
      const references: import('../videoCanvas/api').CanvasMediaMeta[] = [];
      let canvasCreated = false;
      try {
        const { uploadCanvasMedia } = await import('../videoCanvas/api');
        for (const reference of draft.canvasReferences) {
          references.push(
            await uploadCanvasMedia(reference.file, reference.file.name)
          );
        }
        const skillId = draft.creationSkillId;
        const skillDefaults = {
          cinematic: { label: '电影写实', desc: '纪实光影 · 叙事镜头' },
          anime: { label: '二次元', desc: '鲜明线稿 · 动漫质感' },
          cyberpunk: { label: '赛博霓虹', desc: '未来都市 · 高对比' },
          inkWash: { label: '水墨意境', desc: '留白构图 · 东方美学' },
        } as const;
        const defaults = skillDefaults[skillId];
        const skillLabel = t(`videoGeneration.create.skills.${skillId}.label`, {
          defaultValue: defaults.label,
        });
        const skillDescription = t(`videoGeneration.create.skills.${skillId}.desc`, {
          defaultValue: defaults.desc,
        });
        const title =
          draft.creationPrompt.split(/\r?\n/, 1)[0]?.trim().slice(0, 36) ||
          t('videoGeneration.create.canvasTitleFromSkill', {
            skill: skillLabel,
            defaultValue: '{{skill}}创作',
          });
        const id = await createServerBackedCanvasProject(title, {
          prompt: draft.creationPrompt,
          requirement: draft.requirement.trim() || undefined,
          mediaKind: draft.preferences.mediaKind,
          intent: 'creation',
          autoAgent: true,
          skill: {
            id: draft.creationSkillId,
            label: skillLabel,
            description: skillDescription,
            stylePrompt: draft.style,
          },
          preferences: {
            automatic: draft.preferences.automatic,
            aspectRatio: draft.preferences.aspectRatio,
            resolution: draft.preferences.resolution,
            fps: draft.preferences.fps,
            // Canvas nodes expect single-clip seconds (≈4–15), not Agent film length.
            targetDurationSecs: clampDuration(
              draft.preferences.targetDurationSecs,
              CLIP_DURATION_MIN_SECS,
              CLIP_DURATION_MAX_SECS,
              CLIP_DURATION_STEP_SECS
            ) || CLIP_DURATION_DEFAULT_SECS,
            imageModel: draft.preferences.models.image_model || undefined,
            videoModel: draft.preferences.models.video_model || undefined,
          },
          references,
        });
        canvasCreated = true;
        trackFunnelEvent('task_accepted', {
          feature: 'video_generation',
          mode: 'creation',
          skill: draft.creationSkillId,
          project_id: id,
        });
        clearVideoHomeDraft();
        navigate(videoCanvasProjectPath(id));
      } catch (cause) {
        if (isInvalidCloudSessionError(cause)) {
          await logout();
          navigate('/cloud-login');
          return;
        }
        if (!canvasCreated && references.length > 0) {
          const { deleteCanvasMedia } = await import('../videoCanvas/api');
          await Promise.allSettled(
            references.map((reference) => deleteCanvasMedia(reference.media_id))
          );
        }
        message.error(
          `${t('videoCanvas.actions.createFailed', { defaultValue: '创建失败' })}: ${
            cause instanceof Error ? cause.message : String(cause)
          }`
        );
      } finally {
        setCreating(false);
      }
    },
    [creating, logout, message, navigate, t]
  );

  /**
   * Clip generation mode: prompt + optional refs → Canvas video generation task
   * → dedicated clip result page.
   *
   * Uses Canvas API for direct video generation (no Vimax planning workflow).
   */
  const handleCreateGenerate = useCallback(
    async (draft: VideoCreateDraft) => {
      if (creating) return;
      setCreating(true);

      // Store refs to clean up on error
      const uploadedMediaIds: string[] = [];

      try {
        const title =
          draft.creationPrompt.split(/\r?\n/, 1)[0]?.trim().slice(0, 36) ||
          t('videoGeneration.create.generateTitle', {
            defaultValue: '视频生成',
          });

        // 1. Upload reference images to Canvas media storage
        const { uploadCanvasMedia } = await import('../videoCanvas/api');
        const pendingReferences = draft.canvasReferences.filter((ref) => ref.file);
        if (draft.canvasReferences.length > 0 && pendingReferences.length === 0) {
          throw new Error(
            t('videoGeneration.create.upload.referenceFileMissing', {
              defaultValue: '参考图文件已失效（刷新页面后需重新选择图片），请重新添加后再发送。',
            })
          );
        }

        for (const reference of pendingReferences) {
          const media = await uploadCanvasMedia(
            reference.file,
            reference.file.name.replace(/\.[^.]+$/, '').trim() || `参考图${pendingReferences.indexOf(reference) + 1}`
          );
          uploadedMediaIds.push(media.media_id);
        }

        // 2. Create video generation task via Canvas API
        const { createGenerationTask } = await import('../videoCanvas/api');
        const durationSecs =
          clampDuration(
            draft.preferences.targetDurationSecs,
            CLIP_DURATION_MIN_SECS,
            CLIP_DURATION_MAX_SECS,
            CLIP_DURATION_STEP_SECS
          ) || CLIP_DURATION_DEFAULT_SECS;

        const task = await createGenerationTask({
          mode: 'video',
          prompt: draft.creationPrompt,
          model: draft.preferences.models.video_model || undefined,
          resolution: draft.preferences.resolution,
          duration_secs: durationSecs,
          reference_media_ids: uploadedMediaIds,
        });

        trackFunnelEvent('task_accepted', {
          feature: 'video_generation',
          mode: 'generate',
          task_id: task.task_id,
        });

        trackFunnelEvent('render_started', {
          feature: 'video_generation',
          workflow: 'canvas_clip',
          task_id: task.task_id,
          source: 'generate_create',
          duration_secs: durationSecs,
          has_references: pendingReferences.length > 0,
        });

        // Track this direct clip task so the sider can list it. Titles cached
        // now paint the MRU strip before the network resolves.
        rememberVideoGenerationTask(task.task_id, title);

        clearVideoHomeDraft();

        // Navigate to clip result page with task info
        navigate(
          `/video-generation/clip/${encodeURIComponent(task.task_id)}`,
          {
            state: {
              title,
              prompt: draft.creationPrompt,
              taskId: task.task_id,
            },
          }
        );
      } catch (cause) {
        // Clean up uploaded media on error
        if (uploadedMediaIds.length > 0) {
          const { deleteCanvasMedia } = await import('../videoCanvas/api');
          await Promise.allSettled(
            uploadedMediaIds.map((id) => deleteCanvasMedia(id))
          );
        }

        if (isInvalidCloudSessionError(cause)) {
          await logout();
          navigate('/cloud-login');
          return;
        }
        message.error(
          `${t('videoGeneration.create.generateFailed', {
            defaultValue: '视频生成创建失败',
          })}: ${cause instanceof Error ? cause.message : String(cause)}`
        );
      } finally {
        setCreating(false);
      }
    },
    [creating, logout, message, navigate, t]
  );

  const handleCreateBriefing = useCallback(
    async (draft: VideoCreateDraft) => {
      if (creating) return;
      const sourceUrls = draft.sourceUrls
        .split(/[\s,]+/)
        .map((row) => row.trim())
        .filter((url) => /^https?:\/\//i.test(url));
      setCreating(true);
      try {
        const created = await createBriefing({
          intent: draft.sourceText.trim(),
          title: titleForDraft(draft) || undefined,
          format_secs: draft.briefingFormatSecs,
          research_depth: draft.researchDepth,
          time_window_hours: draft.timeWindowHours,
          source_urls: sourceUrls,
          tts_provider_id: draft.briefingTts?.provider_id,
          tts_model: draft.briefingTts?.model,
          tts_voice: draft.briefingTts?.voice ?? undefined,
          image_provider_id: draft.briefingImage?.provider_id,
          image_model: draft.briefingImage?.model,
        });
        trackFunnelEvent('task_accepted', {
          feature: 'video_generation',
          mode: 'briefing',
          workflow: 'news_briefing',
          briefing_id: created.id,
          session_id: created.id,
        });
        trackFunnelEvent('first_task_started', {
          feature: 'video_generation',
          mode: 'briefing',
          workflow: 'news_briefing',
          briefing_id: created.id,
          session_id: created.id,
        });
        try {
          await runBriefing(created.id);
          trackFunnelEvent('render_started', {
            feature: 'video_generation',
            mode: 'briefing',
            workflow: 'news_briefing',
            briefing_id: created.id,
            session_id: created.id,
          });
        } catch {
          // Workspace idle auto-start retries if the first kickoff fails.
        }
        clearVideoHomeDraft();
        navigate(briefingWorkspacePath(created.id));
      } catch (e) {
        if (isInvalidCloudSessionError(e)) {
          await logout();
          navigate('/cloud-login');
          return;
        }
        message.error(
          `${t('videoGeneration.actions.createFailed', { defaultValue: '创建失败' })}: ${
            e instanceof Error ? e.message : String(e)
          }`
        );
      } finally {
        setCreating(false);
      }
    },
    [creating, logout, message, navigate, t]
  );

  const openSession = useCallback(
    (s: SessionSummary) => {
      rememberVideoGenerationSession(s.id, s.title);
      navigate(`/video-generation/${s.id}`);
    },
    [navigate]
  );

  const handleDelete = useCallback(
    async (s: SessionSummary) => {
      if (deletingId) return;
      setDeletingId(s.id);
      try {
        await deleteSession(s.id);
        clearVideoGenerationSessionMemory(s.id);
        setSessions((prev) => prev.filter((x) => x.id !== s.id));
        message.success(t('videoGeneration.actions.deleteOk', { defaultValue: '已删除任务' }));
      } catch (e) {
        message.error(
          `${t('videoGeneration.actions.deleteFailed', { defaultValue: '删除失败' })}: ${
            e instanceof Error ? e.message : String(e)
          }`
        );
      } finally {
        setDeletingId(null);
      }
    },
    [deletingId, message, t]
  );

  // Keep the per-row onDelete prop referentially stable so React.memo on
  // SessionCard can skip re-rendering untouched rows (handleDelete itself
  // changes whenever deletingId/message/t change).
  const handleDeleteRef = useRef<(session: SessionSummary) => void>(() => {});
  useEffect(() => {
    handleDeleteRef.current = handleDelete;
  }, [handleDelete]);

  const onDeleteSession = useCallback((session: SessionSummary) => {
    void handleDeleteRef.current(session);
  }, []);

  const handleDeleteTask = useCallback(
    async (task: GenerationTaskView) => {
      if (deletingId) return;
      setDeletingId(task.task_id);
      try {
        const { deleteGenerationTask } = await import('../videoCanvas/api');
        await deleteGenerationTask(task.task_id);
        clearVideoGenerationSessionMemory(task.task_id);
        setGenerationTasks((prev) => prev.filter((item) => item.task_id !== task.task_id));
        message.success(t('videoGeneration.actions.deleteOk', { defaultValue: '已删除任务' }));
      } catch (e) {
        message.error(
          `${t('videoGeneration.actions.deleteFailed', { defaultValue: '删除失败' })}: ${
            e instanceof Error ? e.message : String(e)
          }`
        );
      } finally {
        setDeletingId(null);
      }
    },
    [deletingId, message, t]
  );

  const handleDeleteTaskRef = useRef<(task: GenerationTaskView) => void>(() => {});
  useEffect(() => {
    handleDeleteTaskRef.current = handleDeleteTask;
  }, [handleDeleteTask]);

  const onDeleteTask = useCallback((task: GenerationTaskView) => {
    void handleDeleteTaskRef.current(task);
  }, []);

  const handleImportProject = useCallback(async () => {
    if (importing || creating) return;
    if (!isDesktopShell()) {
      message.info(
        t('videoGeneration.list.importDesktopOnly', {
          defaultValue: '导入工程仅桌面端可用。',
        })
      );
      return;
    }
    const { dialog } = await import('@/common/adapter/ipcBridge');
    const paths = await dialog.showOpen.invoke({
      properties: ['openFile'],
      filters: [
        {
          name: t('videoGeneration.actions.exportFilter', { defaultValue: 'Flowy 视频工程' }),
          extensions: ['nomivimax'],
        },
      ],
    });
    const source = paths?.[0];
    if (!source) return;
    setImporting(true);
    try {
      const imported = await importSession(source);
      trackFunnelEvent('task_accepted', {
        feature: 'video_generation',
        workflow: imported.workflow,
        session_id: imported.id,
        source: 'project_import',
      });
      message.success(t('videoGeneration.list.importOk', { defaultValue: '工程已导入' }));
      navigate(`/video-generation/${imported.id}`);
    } catch (e) {
      message.error(
        `${t('videoGeneration.list.importFailed', { defaultValue: '导入失败' })}: ${
          e instanceof Error ? e.message : String(e)
        }`
      );
    } finally {
      setImporting(false);
    }
  }, [creating, importing, message, navigate, t]);

  const handleImportCanvasProject = useCallback(async () => {
    if (importing || creating) return;
    if (!isDesktopShell()) {
      message.info(
        t('videoGeneration.list.importDesktopOnly', {
          defaultValue: '导入工程仅桌面端可用。',
        })
      );
      return;
    }
    const { dialog } = await import('@/common/adapter/ipcBridge');
    const paths = await dialog.showOpen.invoke({
      properties: ['openFile'],
      filters: [
        {
          name: t('videoGeneration.actions.exportCanvasFilter', {
            defaultValue: 'Flowy 画布工程',
          }),
          extensions: ['nomiccanvas'],
        },
      ],
    });
    const source = paths?.[0];
    if (!source) return;
    setImporting(true);
    try {
      const { importCanvasProject } = await import('../videoCanvas/api');
      const imported = await importCanvasProject(source);
      trackFunnelEvent('task_accepted', {
        feature: 'video_generation',
        workflow: 'canvas',
        session_id: imported.project_id,
        source: 'project_import',
      });
      message.success(t('videoGeneration.list.importOk', { defaultValue: '工程已导入' }));
      navigate(videoCanvasProjectPath(imported.project_id));
    } catch (e) {
      message.error(
        `${t('videoGeneration.list.importFailed', { defaultValue: '导入失败' })}: ${
          e instanceof Error ? e.message : String(e)
        }`
      );
    } finally {
      setImporting(false);
    }
  }, [creating, importing, message, navigate, t]);

  return (
    <div
      ref={pageScrollRef}
      className={[
        styles.page,
        'flex-1 min-h-0 size-full box-border overflow-y-auto',
        isMobile ? 'px-12px py-12px' : 'px-16px py-24px md:px-36px md:py-32px',
      ].join(' ')}
    >
      {messageHolder}
      <div className='mx-auto flex w-full max-w-1180px box-border flex-col gap-26px'>
        <VideoHomeComposer
          mode={workMode}
          loading={creating}
          onModeChange={handleModeChange}
          onSubmitAgent={(draft) => void handleCreate(draft)}
          onSubmitCreation={(draft) => void handleCreateCanvas(draft)}
          onSubmitGenerate={(draft) => void handleCreateGenerate(draft)}
          onSubmitBriefing={(draft) => void handleCreateBriefing(draft)}
        />

        <section className='flex flex-col gap-12px'>
            <div className='flex flex-wrap items-center justify-between gap-12px'>
              <div>
                <div className='mb-8px'>
                  <SegmentedTabs
                    size='sm'
                    items={listTabItems}
                    activeKey={listTab}
                    onChange={handleListTabChange}
                  />
                </div>
                <h2 className='m-0 text-16px font-650 text-[var(--color-text-1)]'>
                  {listTab === 'tvShow'
                    ? t('videoGeneration.tvShow.title', { defaultValue: 'Flowy TV' })
                    : workMode === 'creation'
                    ? t('videoGeneration.list.creationRecentTitle', {
                        defaultValue: '最近创作',
                      })
                    : workMode === 'generate'
                    ? t('videoGeneration.list.generateRecentTitle', {
                        defaultValue: '最近视频',
                      })
                    : workMode === 'action'
                    ? t('videoGeneration.list.actionRecentTitle', {
                        defaultValue: '动作模仿',
                      })
                    : workMode === 'briefing'
                    ? t('videoGeneration.list.briefingRecentTitle', {
                        defaultValue: '最近资讯播报',
                      })
                    : t('videoGeneration.list.recentTitle', {
                        defaultValue: '最近创作',
                      })}
                </h2>
                <p className='m-0 mt-3px text-12px text-[var(--color-text-3)]'>
                  {listTab === 'tvShow'
                    ? searchParams.get('tvScope') === 'campaign'
                      ? t('videoGeneration.campaign.subtitle', {
                          defaultValue: '参与官方活动，投稿成片，看看获奖作品。',
                        })
                      : t('videoGeneration.tvShow.subtitle', {
                          defaultValue: '浏览社区已上架的作品，或查看你的发布审核状态。',
                        })
                    : workMode === 'creation'
                    ? t('videoGeneration.list.creationRecentSubtitle', {
                        defaultValue: '继续编辑画布，或导入一份分享来的画布工程。',
                      })
                    : workMode === 'generate'
                    ? t('videoGeneration.list.generateRecentSubtitle', {
                        defaultValue: '继续视频创作。',
                      })
                    : workMode === 'action'
                    ? t('videoGeneration.list.actionRecentSubtitle', {
                        defaultValue: '继续动作模仿项目。',
                      })
                    : workMode === 'briefing'
                    ? t('videoGeneration.list.briefingRecentSubtitle', {
                        defaultValue: '继续编辑拍脚本、核对引用或导出成片。',
                      })
                    : t('videoGeneration.list.recentSubtitle', {
                        defaultValue: '继续分镜、渲染或查看已经完成的影片。',
                      })}
                </p>
              </div>
              {listTab === 'recent' ? (
                <div className='flex flex-wrap items-center gap-10px'>
                  {workMode === 'briefing' ? null : (
                  <Button
                    type='outline'
                    size='small'
                    loading={importing}
                    disabled={creating || importing}
                    onClick={() =>
                      void (workMode === 'creation'
                        ? handleImportCanvasProject()
                        : handleImportProject())
                    }
                  >
                    <span className='inline-flex items-center gap-4px'>
                      <Upload theme='outline' size={14} fill='currentColor' />
                      {t('videoGeneration.list.importProject', {
                        defaultValue: '导入工程',
                      })}
                    </span>
                  </Button>
                  )}
                  {(workMode === 'briefing'
                    ? briefingSessions.length > 0
                    : workMode !== 'creation' && sessions.length > 0) && !error ? (
                    <div className='flex w-220px items-center gap-8px rd-10px border border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] px-11px py-7px'>
                      <Search
                        theme='outline'
                        size={14}
                        className='flex-none text-[var(--color-text-3)]'
                      />
                      <input
                        className='w-full border-none bg-transparent text-13px text-[var(--color-text-1)] outline-none font-[inherit] placeholder:text-[var(--color-text-3)]'
                        placeholder={t('videoGeneration.list.searchPlaceholder', {
                          defaultValue: '搜索项目...',
                        })}
                        value={searchQuery}
                        onChange={(event) => setSearchQuery(event.target.value)}
                      />
                    </div>
                  ) : null}
                </div>
              ) : null}
            </div>

            <div>
              {listTab === 'tvShow' ? (
                <Suspense
                  fallback={
                    <div className='flex justify-center py-38px'>
                      <Spin />
                    </div>
                  }
                >
                  <TvShowPanel enabled />
                </Suspense>
              ) : workMode === 'creation' ? (
                <Suspense
                  fallback={
                    <div className='flex justify-center py-38px'>
                      <Spin />
                    </div>
                  }
                >
                  <CanvasProjectGallery embedded />
                </Suspense>
              ) : workMode === 'generate' ? (
                // Video generation mode - show generation tasks
                loadingTasks ? (
                  <div className='flex justify-center py-38px'>
                    <Spin />
                  </div>
                ) : generationTasks.length === 0 ? (
                  <div className='flex items-center gap-12px rd-14px border border-dashed border-[var(--color-border-2)] bg-[var(--color-fill-1)] px-16px py-18px'>
                    <span className='flex h-38px w-38px shrink-0 items-center justify-center rd-11px bg-[rgba(var(--primary-6),0.1)] text-[rgb(var(--primary-6))]'>
                      <VideoOne theme='outline' size={19} fill='currentColor' />
                    </span>
                    <div>
                      <div className='text-13px font-600 text-[var(--color-text-1)]'>
                        {t('videoGeneration.list.generateEmpty.title', {
                          defaultValue: '你的第一个视频从这里开始',
                        })}
                      </div>
                      <div className='mt-2px text-12px text-[var(--color-text-3)]'>
                        {t('videoGeneration.list.generateEmpty.desc', {
                          defaultValue: '输入描述，上方开始生成视频。',
                        })}
                      </div>
                    </div>
                  </div>
                ) : (
                  <>
                    <Suspense
                      fallback={
                        <div className='flex justify-center py-38px'>
                          <Spin />
                        </div>
                      }
                    >
                      <div
                        className='grid gap-12px'
                        style={{
                          gridTemplateColumns:
                            'repeat(auto-fill, minmax(min(300px, 100%), 1fr))',
                        }}
                      >
                        {displayedTasks.map((task) => (
                          <GenerationTaskCard
                            key={task.task_id}
                            task={task}
                            onDelete={onDeleteTask}
                            deleting={deletingId === task.task_id}
                          />
                        ))}
                      </div>
                    </Suspense>
                    {displayedTasks.length === 0 && (
                      <div className='flex flex-col items-center gap-8px py-40px text-[var(--color-text-3)] text-13px'>
                        {t('videoGeneration.list.filterEmpty', {
                          defaultValue: '没有匹配的任务',
                        })}
                      </div>
                    )}
                  </>
                )
              ) : workMode === 'briefing' ? (
                <div>
                  {error ? (
                    <Result
                      status='error'
                      title={t('videoGeneration.list.loadError', {
                        defaultValue: '加载失败',
                      })}
                      subTitle={error}
                      extra={
                        <Button onClick={() => void refreshBriefings()}>
                          {t('videoGeneration.list.retry', { defaultValue: '重试' })}
                        </Button>
                      }
                    />
                  ) : loading ? (
                    <div className='flex justify-center py-38px'>
                      <Spin />
                    </div>
                  ) : briefingSessions.length === 0 ? (
                    <div className='flex items-center gap-12px rd-14px border border-dashed border-[var(--color-border-2)] bg-[var(--color-fill-1)] px-16px py-18px'>
                      <span className='flex h-38px w-38px shrink-0 items-center justify-center rd-11px bg-[rgba(var(--primary-6),0.1)] text-[rgb(var(--primary-6))]'>
                        <VideoOne theme='outline' size={19} fill='currentColor' />
                      </span>
                      <div>
                        <div className='text-13px font-600 text-[var(--color-text-1)]'>
                          {t('videoGeneration.list.briefingEmpty.title', {
                            defaultValue: '你的第一条资讯播报从上方开始',
                          })}
                        </div>
                        <div className='mt-2px text-12px text-[var(--color-text-3)]'>
                          {t('videoGeneration.list.briefingEmpty.desc', {
                            defaultValue: '写下话题并贴上至少两个独立来源，才会进入调研与口播。',
                          })}
                        </div>
                      </div>
                    </div>
                  ) : (
                    <>
                      <div
                        className='grid gap-12px'
                        style={{
                          gridTemplateColumns:
                            'repeat(auto-fill, minmax(min(300px, 100%), 1fr))',
                        }}
                      >
                        {displayedBriefings.map((session) => (
                          <button
                            key={session.id}
                            type='button'
                            className='flex flex-col gap-8px rd-14px border border-solid border-[var(--color-border-2)] bg-[var(--color-bg-2)] px-16px py-16px text-left'
                            onClick={() => navigate(briefingWorkspacePath(session.id))}
                          >
                            <strong className='text-14px text-[var(--color-text-1)]'>
                              {session.title ||
                                t('videoGeneration.list.untitled', { defaultValue: '未命名任务' })}
                            </strong>
                            <span className='text-12px text-[var(--color-text-3)]'>
                              {session.status} · {session.stage}
                            </span>
                          </button>
                        ))}
                      </div>
                      {displayedBriefings.length === 0 ? (
                        <div className='flex flex-col items-center gap-8px py-40px text-[var(--color-text-3)] text-13px'>
                          {t('videoGeneration.list.filterEmpty', {
                            defaultValue: '没有匹配的任务',
                          })}
                        </div>
                      ) : null}
                    </>
                  )}
                </div>
              ) : (
                // Agent/Action modes - show sessions
                <div>
                  {error ? (
                    <Result
                      status='error'
                      title={t('videoGeneration.list.loadError', {
                        defaultValue: '加载失败',
                      })}
                      subTitle={error}
                      extra={
                        <Button onClick={() => void refresh()}>
                          {t('videoGeneration.list.retry', { defaultValue: '重试' })}
                        </Button>
                      }
                    />
                  ) : loading ? (
                    <div className='flex justify-center py-38px'>
                      <Spin />
                    </div>
                  ) : sessions.length === 0 ? (
                    <div className='flex items-center gap-12px rd-14px border border-dashed border-[var(--color-border-2)] bg-[var(--color-fill-1)] px-16px py-18px'>
                      <span className='flex h-38px w-38px shrink-0 items-center justify-center rd-11px bg-[rgba(var(--primary-6),0.1)] text-[rgb(var(--primary-6))]'>
                        <VideoOne theme='outline' size={19} fill='currentColor' />
                      </span>
                      <div>
                        <div className='text-13px font-600 text-[var(--color-text-1)]'>
                          {t('videoGeneration.list.empty.title', {
                            defaultValue: '你的第一支影片从上方开始',
                          })}
                        </div>
                        <div className='mt-2px text-12px text-[var(--color-text-3)]'>
                          {t('videoGeneration.list.empty.desc', {
                            defaultValue: '写下一个画面或故事，Flowy 会先给你一版可编辑分镜。',
                          })}
                        </div>
                      </div>
                    </div>
                  ) : (
                    <>
                      <Suspense
                        fallback={
                          <div className='flex justify-center py-38px'>
                            <Spin />
                          </div>
                        }
                      >
                        <div
                          className='grid gap-12px'
                          style={{
                            gridTemplateColumns:
                              'repeat(auto-fill, minmax(min(300px, 100%), 1fr))',
                          }}
                        >
                          {displayed.map((session) => (
                            <SessionCard
                              key={session.id}
                              session={session}
                              onOpen={openSession}
                              onDelete={onDeleteSession}
                              deleting={deletingId === session.id}
                            />
                          ))}
                        </div>
                      </Suspense>
                      {displayed.length === 0 && (
                        <div className='flex flex-col items-center gap-8px py-40px text-[var(--color-text-3)] text-13px'>
                          {t('videoGeneration.list.filterEmpty', {
                            defaultValue: '没有匹配的任务',
                          })}
                        </div>
                      )}
                    </>
                  )}
                </div>
              )}
            </div>
          </section>
      </div>
    </div>
  );
};

export default VideoGenerationListPage;
