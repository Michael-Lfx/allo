import { useCallback, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { useArcoMessage } from '@renderer/utils/ui/useArcoMessage';
import { useCloudAuth } from '@renderer/hooks/context/CloudAuthContext';
import { useCredits } from '@renderer/hooks/context/CreditsContext';
import { isBackendHttpError, isInvalidCloudSessionError } from '@/common/adapter/httpBridge';
import { trackFunnelEvent, trackLowCreditBalance } from '@renderer/utils/analytics/productFunnel';
import {
  createSession,
  renderSession,
  uploadActionAssets,
  uploadCameo,
} from '../videoGeneration/api';
import { briefingWorkspacePath, createBriefing, runBriefing } from '../videoGeneration/briefing/api';
import type { VideoCreateDraft } from '../videoGeneration/home/types';
import {
  buildBriefingCreateInput,
  buildGenerateTaskPayload,
  buildHomeVideoGenerateRequest,
  buildVideoSessionCreateInput,
} from '../videoGeneration/home/submitDtos';
import { clampClipDurationForModel } from '../videoGeneration/durationBounds';
import { lookById } from '../videoGeneration/styleCatalog/looks';

export { buildBriefingCreateInput, buildHomeVideoGenerateRequest, buildVideoSessionCreateInput };
import { clearVideoHomeDraft } from '../videoGeneration/home/homeDraft';
import { prefetchCanvasWorkspace } from '../videoGeneration/prefetch';
import { isInsufficientCreditsError } from '../videoGeneration/creditsError';
import { formatClipOperationError } from '../videoGeneration/clipFailure';
import { videoCanvasProjectPath } from '../videoCanvas/routes';
import {
  createGenerationTask,
  deleteCanvasMedia,
  listGenerationTasks,
  uploadCanvasMedia,
  type GenerationTaskView,
} from '../videoCanvas/api';
import { resolveLookIdentity } from '../videoGeneration/styleCatalog/lookIdentity';
import {
  rememberVideoGenerationBriefing,
  rememberVideoGenerationCanvas,
  rememberVideoGenerationSession,
  rememberVideoGenerationTask,
} from '../videoGeneration/routeMemory';

/** A stuck create POST must not pin the home send guard forever. */
const HOME_VIDEO_CREATE_TIMEOUT_MS = 60_000;
/** A stalled reference upload must not leave the composer stuck in `creating`. */
const HOME_VIDEO_UPLOAD_TIMEOUT_MS = 120_000;
const HOME_VIDEO_CLAIM_PAGE_SIZE = 50;
const HOME_VIDEO_CLAIM_MAX_TASKS = 500;

function withHomeVideoTimeout<T>(
  promise: Promise<T>,
  timeoutMs: number,
  message = 'home video operation timed out',
): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = window.setTimeout(() => reject(new Error(message)), timeoutMs);
    promise.then(
      (value) => {
        window.clearTimeout(timer);
        resolve(value);
      },
      (error) => {
        window.clearTimeout(timer);
        reject(error);
      },
    );
  });
}

export function isDefinitelyNotCreatedCreateError(error: unknown): boolean {
  return isBackendHttpError(error) && error.status >= 400 && error.status < 500;
}

async function listHomeVideoClaimCandidates(): Promise<GenerationTaskView[]> {
  const tasks: GenerationTaskView[] = [];
  let offset = 0;
  while (tasks.length < HOME_VIDEO_CLAIM_MAX_TASKS) {
    const page = await listGenerationTasks(HOME_VIDEO_CLAIM_PAGE_SIZE, offset, {
      standalone: true,
    });
    const batch = page.tasks ?? [];
    tasks.push(...batch);
    offset += batch.length;
    if (batch.length === 0 || offset >= (page.total ?? 0)) break;
  }
  return tasks;
}

type HomeVideoGenerateRecovery = ReturnType<typeof buildHomeVideoGenerateRequest> & {
  uploadedMediaIds: string[];
};

/** A task can only be claimed when its reference-media set uniquely matches. */
export function findUniqueClaimTask<T extends { reference_media_ids?: string[] | null }>(
  tasks: T[],
  uploadedMediaIds: string[],
): T | null {
  if (uploadedMediaIds.length === 0) return null;
  const wanted = [...uploadedMediaIds].sort().join(',');
  const matches = tasks.filter(
    (task) => [...(task.reference_media_ids ?? [])].sort().join(',') === wanted,
  );
  return matches.length === 1 ? matches[0] : null;
}

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

export function useHomeVideoSubmit() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [message, messageHolder] = useArcoMessage();
  const { logout } = useCloudAuth();
  const { balance } = useCredits();
  const [creating, setCreating] = useState(false);
  const [generateRecovery, setGenerateRecovery] = useState<HomeVideoGenerateRecovery | null>(null);
  const [generateRecoveryChecking, setGenerateRecoveryChecking] = useState(false);
  const generateAttemptRef = useRef<
    (HomeVideoGenerateRecovery & { postIssued: boolean }) | null
  >(null);
  const generateRecoveryCheckingRef = useRef(false);

  const handleCreate = useCallback(
    async (draft: VideoCreateDraft) => {
      if (creating) return;
      setCreating(true);
      try {
        const created = await createSession(buildVideoSessionCreateInput(draft));
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
              error_message: raw.slice(0, 256),
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
          if (isInsufficientCreditsError(raw)) {
            trackLowCreditBalance({ source: 'video_launch', balance });
          }
          navigate(`/video-generation/${created.id}`, {
            state: { launchDraft: draft, launchError: true },
          });
          rememberVideoGenerationSession(created.id, created.title);
          return;
        }
        rememberVideoGenerationSession(created.id, created.title);
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
    [creating, logout, navigate, message, t, balance]
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
          references.push({
            ...(await uploadCanvasMedia(reference.file, reference.file.name)),
            ...(reference.subjectKind ? { subjectKind: reference.subjectKind } : {}),
            ...(reference.subjectName?.trim() ? { subjectName: reference.subjectName.trim() } : {}),
          });
        }
        const identity = resolveLookIdentity({
          stylePrompt: draft.style,
          creationSkillId: draft.style.trim() ? draft.creationSkillId : undefined,
        });
        const look =
          (identity?.vimaxKey && lookById.get(identity.vimaxKey)) ||
          (identity ? lookById.get(identity.canvasPresetId) : undefined);
        const lookLabel = look
          ? t(look.labelKey, { defaultValue: look.defaultLabel })
          : identity?.canvasTitle ||
            t('videoGeneration.looks.mountButton', { defaultValue: '画风' });
        const title =
          draft.creationPrompt.split(/\r?\n/, 1)[0]?.trim().slice(0, 36) ||
          t('videoGeneration.create.canvasTitleFromSkill', {
            skill: lookLabel,
            defaultValue: '{{skill}}创作',
          });
        const id = await createServerBackedCanvasProject(title, {
          prompt: draft.creationPrompt,
          requirement: draft.requirement.trim() || undefined,
          mediaKind: draft.preferences.mediaKind,
          intent: 'creation',
          autoAgent: true,
          ...(identity
            ? {
                skill: {
                  id: identity.vimaxKey ?? identity.canvasPresetId ?? draft.creationSkillId,
                  label: lookLabel,
                  description: look?.defaultDescription || lookLabel,
                  stylePrompt: identity.modelPrompt ?? draft.style,
                  stylePresetId: identity.canvasPresetId,
                },
              }
            : {}),
          preferences: {
            automatic: draft.preferences.automatic,
            aspectRatio: draft.preferences.aspectRatio,
            resolution: draft.preferences.resolution,
            fps: draft.preferences.fps,
            // Canvas nodes expect a single-clip duration for the selected video model.
            targetDurationSecs: clampClipDurationForModel(
              draft.preferences.models.video_model,
              draft.preferences.targetDurationSecs
            ),
            imageModel: draft.preferences.models.image_model || undefined,
            videoModel: draft.preferences.models.video_model || undefined,
          },
          references,
        });
        canvasCreated = true;
        rememberVideoGenerationCanvas(id, title);
        trackFunnelEvent('task_accepted', {
          feature: 'video_generation',
          mode: 'creation',
          workflow: 'canvas',
          session_id: id,
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

  const handleCreateBlankCanvas = useCallback(async () => {
    if (creating) return;
    setCreating(true);
    prefetchCanvasWorkspace();
    const title = t('videoGeneration.create.gallery.untitled', {
      defaultValue: '未命名画布',
    });
    try {
      const id = await createServerBackedCanvasProject(title);
      rememberVideoGenerationCanvas(id, title);
      trackFunnelEvent('task_accepted', {
        feature: 'video_generation',
        mode: 'creation',
        workflow: 'canvas',
        session_id: id,
        project_id: id,
        source: 'blank_canvas',
      });
      navigate(videoCanvasProjectPath(id));
    } catch (cause) {
      if (isInvalidCloudSessionError(cause)) {
        await logout();
        navigate('/cloud-login');
        return;
      }
      message.error(
        t('videoGeneration.create.gallery.createFailed', {
          error: cause instanceof Error ? cause.message : String(cause),
          defaultValue: '创建失败：{{error}}',
        })
      );
    } finally {
      setCreating(false);
    }
  }, [creating, logout, message, navigate, t]);

  const openGenerateTask = useCallback(
    async (taskId: string, title: string, prompt: string): Promise<boolean> => {
      try {
        rememberVideoGenerationTask(taskId, title);
        await navigate(`/video-generation/clip/${encodeURIComponent(taskId)}`, {
          state: { title, prompt, taskId },
        });
        return true;
      } catch (navigationError) {
        console.error('Failed to open the video task route:', navigationError);
        message.error(
          t('guid.home.videoOpenTaskFailed'),
        );
        return false;
      }
    },
    [message, navigate, t],
  );

  const resolveGenerateRecovery = useCallback(async () => {
    const pending = generateRecovery;
    if (!pending || generateRecoveryCheckingRef.current) return;
    if (pending.uploadedMediaIds.length === 0) {
      message.warning(t('common.unknownError'));
      return;
    }
    generateRecoveryCheckingRef.current = true;
    setGenerateRecoveryChecking(true);
    try {
      const tasks = await listHomeVideoClaimCandidates();
      const task = findUniqueClaimTask(tasks, pending.uploadedMediaIds);
      if (!task) {
        message.warning(t('common.unknownError'));
        return;
      }
      setGenerateRecovery(null);
      clearVideoHomeDraft();
      await openGenerateTask(task.task_id, pending.title, pending.prompt);
    } catch (cause) {
      message.error(
        formatClipOperationError(cause, t, {
          key: 'videoGeneration.list.loadError',
          defaultValue: '加载失败',
        }).message,
      );
    } finally {
      generateRecoveryCheckingRef.current = false;
      setGenerateRecoveryChecking(false);
    }
  }, [generateRecovery, message, openGenerateTask, t]);

  /**
   * Generate mode: prompt + optional refs -> Canvas video task -> clip result.
   * Keeps the home R3 contract: once the POST is issued, an unknown outcome can
   * only be resolved by looking up the task, never by issuing a second POST.
   */
  const handleCreateGenerate = useCallback(
    async (draft: VideoCreateDraft) => {
      if (generateRecovery) {
        void resolveGenerateRecovery();
        return;
      }
      if (creating) return;
      const pendingReferences = draft.canvasReferences.filter((reference) => reference.file);
      if (draft.canvasReferences.length > 0 && pendingReferences.length === 0) {
        message.error(
          t('videoGeneration.create.upload.referenceFileMissing', {
            defaultValue: '参考图文件已失效，请重新添加后再发送。',
          }),
        );
        return;
      }
      const request = buildHomeVideoGenerateRequest(draft);
      if (!request.model) {
        message.warning(
          t('videoGeneration.create.modelRequiredAction', {
            defaultValue: 'A video model is required before generating.',
          }),
        );
        return;
      }
      setCreating(true);
      const attempt = {
        ...request,
        title:
          request.title ||
          t('videoGeneration.create.generateTitle', { defaultValue: '视频生成' }),
        uploadedMediaIds: [] as string[],
        postIssued: false,
      };
      generateAttemptRef.current = attempt;
      try {
        for (const reference of pendingReferences) {
          const media = await withHomeVideoTimeout(
            uploadCanvasMedia(
              reference.file,
              reference.file.name.replace(/\.[^.]+$/, '').trim() ||
                `reference-${attempt.uploadedMediaIds.length + 1}`,
            ),
            HOME_VIDEO_UPLOAD_TIMEOUT_MS,
            'home video reference upload timed out',
          );
          attempt.uploadedMediaIds.push(media.media_id);
        }
        attempt.postIssued = true;
        const task = await withHomeVideoTimeout(
          createGenerationTask(buildGenerateTaskPayload(draft, attempt.uploadedMediaIds)),
          HOME_VIDEO_CREATE_TIMEOUT_MS,
        );
        generateAttemptRef.current = null;
        clearVideoHomeDraft();
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
          duration_secs: attempt.durationSecs,
          has_references: pendingReferences.length > 0,
        });
        await openGenerateTask(task.task_id, attempt.title, attempt.prompt);
      } catch (cause) {
        const pending = generateAttemptRef.current;
        generateAttemptRef.current = null;
        if (isInvalidCloudSessionError(cause)) {
          if (pending) {
            await Promise.allSettled(
              pending.uploadedMediaIds.map((mediaId) => deleteCanvasMedia(mediaId)),
            );
          }
          await logout();
          navigate('/cloud-login');
          return;
        }
        if (!pending) return;
        if (!pending.postIssued || isDefinitelyNotCreatedCreateError(cause)) {
          await Promise.allSettled(
            pending.uploadedMediaIds.map((mediaId) => deleteCanvasMedia(mediaId)),
          );
          const failure = formatClipOperationError(cause, t, {
            key: 'videoGeneration.create.generateFailed',
            defaultValue: '视频生成创建失败',
          });
          message.error(failure.message);
          if (failure.credits) {
            trackLowCreditBalance({ source: 'video_launch', balance });
          }
          return;
        }
        // 5xx / timeout / disconnect: the task may exist. Keep the attempt and
        // block a second POST until a unique media set confirms the task.
        setGenerateRecovery({ ...pending, uploadedMediaIds: [...pending.uploadedMediaIds] });
        message.warning(t('common.unknownError'));
      } finally {
        setCreating(false);
      }
    },
    [
      balance,
      creating,
      generateRecovery,
      logout,
      message,
      navigate,
      openGenerateTask,
      resolveGenerateRecovery,
      t,
    ],
  );

  const handleCreateBriefing = useCallback(
    async (draft: VideoCreateDraft) => {
      if (creating) return;
      setCreating(true);
      try {
        const created = await createBriefing(buildBriefingCreateInput(draft));
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
        rememberVideoGenerationBriefing(created.id, created.title);
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

  return {
    creating,
    messageHolder,
    generateRecovery,
    generateRecoveryChecking,
    resolveGenerateRecovery,
    handleCreate,
    handleCreateCanvas,
    handleCreateBlankCanvas,
    handleCreateGenerate,
    handleCreateBriefing,
  };
}
