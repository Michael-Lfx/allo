/**
 * Allo adapter: open-ai-canvas task-center -> /api/video-canvas/tasks.
 */

import { generationErrorMessage } from '@oc/lib/generation-error';
import {
  normalizeMiniMaxH3Duration,
  normalizeMiniMaxH3Ratio,
} from '@oc/lib/minimax-h3-video';
import { isMiniMaxH3VideoModel } from '@renderer/services/videoModelCapabilities';
import { canonicalizeVideoResolution } from '@oc/lib/canvas-video-resolution';
import { resourceFileUrl, resourceIdFromStorageKey, resourceStorageKey } from '@oc/services/api/resources';
import { hasExplicitVideoFrames, resolveVideoImageReferences, shouldSubmitVideoImagesAsReferences } from '@oc/services/api/video-reference-roles';
import { modelOptionName } from '@oc/stores/use-config-store';
import {
  cancelGenerationTask as alloCancel,
  createGenerationTask as alloCreate,
  getGenerationTask as alloGet,
  type CreateGenerationBody,
  type GenerationTaskView,
} from '@renderer/pages/videoCanvas/api';

export type BackendEnvelope<T> = {
  code: number;
  data: T;
  msg: string;
};

export type TaskStatus = 'queued' | 'running' | 'succeeded' | 'failed' | 'cancelled';
export type TaskBillingStatus = 'reserved' | 'running' | 'settled' | 'refunded' | 'uncertain';
export type ProviderCancelStatus = 'requested' | 'confirmed' | 'uncertain';
export type AgentSessionStatus = 'active' | 'completed' | 'failed';

export type GenerationTask = {
  id: string;
  sessionId?: string;
  projectId?: string;
  type: string;
  status: TaskStatus;
  progress?: number;
  stage?: string;
  prompt: string;
  operation?: string;
  provider?: string;
  model?: string;
  providerRequestId?: string;
  providerCancelStatus?: ProviderCancelStatus;
  providerCancelError?: string;
  providerCancelAttempts?: number;
  providerCancelRequestedAt?: string;
  providerCancelledAt?: string;
  errorCode?: string;
  previewUrl?: string;
  previewKind?: 'image' | 'video';
  inputJson?: string;
  resultJson?: string;
  textDraft?: string;
  error?: string;
  attempts: number;
  startedAt?: string;
  completedAt?: string;
  createdAt: string;
  updatedAt: string;
  billing?: {
    amountMicrocredits: number;
    status: TaskBillingStatus;
  };
  clientContext?: {
    conversationId: string;
    messageId: string;
    batchIndex?: number;
    batchCount?: number;
  };
  created_at?: string;
  updated_at?: string;
};

export type ProviderTaskQueryResult = {
  task: GenerationTask;
  providerStatus: string;
  recovered: boolean;
  billingSettled: boolean;
};

export type TaskTextDelta = {
  id: string;
  taskId: string;
  sequence: number;
  content: string;
  byteCount: number;
  createdAt: string;
  expiresAt: string;
};

export type TaskTextReplay = {
  deltas: TaskTextDelta[];
  textDraft?: string;
  finalText?: string;
  complete: boolean;
};

export type AgentSession = {
  id: string;
  projectId?: string;
  status: AgentSessionStatus;
  prompt: string;
  canvasSnapshotJson?: string;
  canvasOpsJson?: string;
  createdAt: string;
  updatedAt: string;
};

export type AgentMessage = {
  id: string;
  sessionId: string;
  role: 'user' | 'assistant' | 'system' | 'tool' | string;
  content: string;
  payload?: string;
  createdAt: string;
};

export type TaskResult = {
  id: string;
  taskId: string;
  sessionId?: string;
  kind: string;
  url?: string;
  payload?: string;
  createdAt: string;
};

export type SessionFile = {
  id: string;
  sessionId: string;
  fileName: string;
  mimeType: string;
  size: number;
  createdAt: string;
};

export type TaskLog = {
  id: string;
  taskId: string;
  level: 'info' | 'warn' | 'error' | string;
  message: string;
  payload?: string;
  createdAt: string;
};

export type AgentSessionDetail = {
  session: AgentSession;
  messages: AgentMessage[];
  tasks: GenerationTask[];
  results: TaskResult[];
};

export type CreateSessionInput = {
  projectId?: string;
  prompt: string;
  canvasSnapshot?: Record<string, unknown>;
  references?: string[];
  projectStyle?: { presetId: string; title: string; prompt: string };
  characters?: Array<{
    assetId: string;
    versionId: string;
    name: string;
    definition: Record<string, unknown>;
  }>;
  config?: Record<string, unknown>;
};

export type CreateTaskInput = {
  sessionId?: string;
  projectId?: string;
  type?: string;
  operation?: string;
  prompt: string;
  provider?: string;
  model?: string;
  input?: Record<string, unknown>;
};

/** In-memory task index (allo has no list endpoint yet). */
const taskIndex = new Map<string, GenerationTask>();

function mapStatus(status: GenerationTaskView['status']): TaskStatus {
  if (status === 'canceled') return 'cancelled';
  return status;
}

function isoFromMs(ms: number | undefined): string {
  return new Date(ms || Date.now()).toISOString();
}

function buildResultJson(view: GenerationTaskView): string | undefined {
  if (!view.result_media_id || view.status !== 'succeeded') return undefined;
  const mediaId = view.result_media_id;
  const url = resourceFileUrl(mediaId);
  const storageKey = resourceStorageKey(mediaId);
  const mode = String(view.mode || '').toLowerCase();
  const isVideo = mode.includes('video') || mode === 't2v' || mode === 'i2v';
  if (isVideo) {
    return JSON.stringify({
      mode: 'video',
      video: { dataUrl: url, storageKey, mimeType: 'video/mp4' },
    });
  }
  return JSON.stringify({
    mode: 'image',
    images: [{ dataUrl: url, storageKey, mimeType: 'image/png' }],
  });
}

export function mapAlloTask(view: GenerationTaskView, extra?: Partial<GenerationTask>): GenerationTask {
  const previewUrl = view.result_media_id ? resourceFileUrl(view.result_media_id) : undefined;
  const mode = String(view.mode || '').toLowerCase();
  const isVideo = mode.includes('video') || mode === 't2v' || mode === 'i2v';
  const task: GenerationTask = {
    ...extra,
    id: view.task_id,
    projectId: extra?.projectId || view.project_id || undefined,
    type: extra?.type || `canvas_${isVideo ? 'video' : 'image'}`,
    status: mapStatus(view.status),
    progress: Math.round((view.progress || 0) * 100),
    prompt: view.prompt || extra?.prompt || '',
    operation: extra?.operation,
    model: view.model || extra?.model,
    previewUrl,
    previewKind: previewUrl ? (isVideo ? 'video' : 'image') : undefined,
    resultJson: buildResultJson(view),
    error: view.error || undefined,
    attempts: 1,
    createdAt: isoFromMs(view.created_at),
    updatedAt: isoFromMs(view.updated_at),
    created_at: isoFromMs(view.created_at),
    updated_at: isoFromMs(view.updated_at),
    inputJson: extra?.inputJson,
  };
  taskIndex.set(task.id, task);
  return task;
}

/**
 * Map task-center reference payloads onto allo generation media ids.
 *
 * Video image-to-video may promote the first reference image to `first_frame`
 * when no explicit frame is set. Named start/end frame node ids win over that
 * fallback. Image / img2img must keep every reference in `reference_media_ids`.
 */
export function collectMediaIds(
  input?: Record<string, unknown>,
  options?: { promoteFirstImageToFrame?: boolean },
): {
  referenceIds: string[];
  firstFrameId?: string;
  lastFrameId?: string;
} {
  if (!input) return { referenceIds: [] };
  const refs: string[] = [];
  const pushKey = (storageKey?: unknown) => {
    if (typeof storageKey !== 'string') return;
    const id = resourceIdFromStorageKey(storageKey);
    if (id) refs.push(id);
  };
  const images = Array.isArray(input.referenceImages) ? input.referenceImages : [];
  const videos = Array.isArray(input.referenceVideos) ? input.referenceVideos : [];
  const audios = Array.isArray(input.referenceAudios) ? input.referenceAudios : [];
  for (const item of [...images, ...videos, ...audios]) {
    if (item && typeof item === 'object') pushKey((item as { storageKey?: string }).storageKey);
  }
  const metadata =
    input.metadata && typeof input.metadata === 'object'
      ? (input.metadata as Record<string, unknown>)
      : {};
  const videoOptions = {
    videoEditOperation: typeof metadata.videoEditOperation === 'string' ? metadata.videoEditOperation : undefined,
    videoStartFrameNodeId: typeof metadata.videoStartFrameNodeId === 'string' ? metadata.videoStartFrameNodeId : undefined,
    videoEndFrameNodeId: typeof metadata.videoEndFrameNodeId === 'string' ? metadata.videoEndFrameNodeId : undefined,
  };
  // Named start/end frames only apply to video. The OA helper's index fallback
  // (1 image = first frame, 2 images = first+last) is Yingce creation-page
  // semantics — canvas keeps "first connected image becomes first_frame, rest
  // stay references" unless the user named the frames.
  const namedFrames = Boolean(options?.promoteFirstImageToFrame) && hasExplicitVideoFrames(videoOptions);
  const imageList = images.flatMap((item) => {
    if (!item || typeof item !== 'object') return [];
    const image = item as { id?: string; storageKey?: string };
    return typeof image.id === 'string' ? [image as { id: string; storageKey?: string }] : [];
  });
  const bindAsReferences = shouldSubmitVideoImagesAsReferences(videoOptions, imageList.length);
  const rolePlan = namedFrames && !bindAsReferences
    ? resolveVideoImageReferences(imageList, videoOptions)
    : [];
  const roleFirst = rolePlan.find((item) => item.role === 'first_frame');
  const roleLast = rolePlan.find((item) => item.role === 'last_frame');
  const metadataFirstFrame =
    (typeof metadata.firstFrameMediaId === 'string' && metadata.firstFrameMediaId) ||
    (typeof metadata.first_frame_media_id === 'string' && metadata.first_frame_media_id) ||
    undefined;
  if (bindAsReferences) {
    return { referenceIds: refs, firstFrameId: undefined, lastFrameId: undefined };
  }
  const firstFrameId =
    metadataFirstFrame ||
    (roleFirst ? resourceIdFromStorageKey(roleFirst.image.storageKey) : undefined) ||
    (options?.promoteFirstImageToFrame &&
    !namedFrames &&
    !metadataFirstFrame &&
    images[0] &&
    typeof images[0] === 'object'
      ? resourceIdFromStorageKey((images[0] as { storageKey?: string }).storageKey) ||
        undefined
      : undefined);
  const lastFrameId =
    (typeof metadata.lastFrameMediaId === 'string' && metadata.lastFrameMediaId) ||
    (typeof metadata.last_frame_media_id === 'string' && metadata.last_frame_media_id) ||
    (roleLast ? resourceIdFromStorageKey(roleLast.image.storageKey) : undefined) ||
    undefined;
  const referenceIds = refs.filter((id) => id !== firstFrameId && id !== lastFrameId);
  return { referenceIds, firstFrameId: firstFrameId || undefined, lastFrameId: lastFrameId || undefined };
}

export function resolveAlloGenerationMode(input: CreateTaskInput): string {
  const payload = input.input || {};
  const modeRaw = String(payload.mode || input.type || 'image').replace(/^canvas_/, '');
  const operation = String(input.operation || payload.operation || modeRaw);
  let mode = modeRaw;
  if (mode === 't2i' || mode === 'i2i' || mode.includes('image')) mode = 'image';
  if (mode === 't2v' || mode === 'i2v' || mode.includes('video')) mode = 'video';
  if (operation === 'image_to_video' || operation === 'text_to_video' || operation === 'reference_to_video') mode = 'video';
  return mode;
}

export function alloBodyFromCreateInput(input: CreateTaskInput): CreateGenerationBody {
  const payload = input.input || {};
  const mode = resolveAlloGenerationMode(input);
  const isVideo = mode === 'video';

  const config =
    payload.config && typeof payload.config === 'object'
      ? (payload.config as Record<string, unknown>)
      : {};
  const metadata =
    payload.metadata && typeof payload.metadata === 'object'
      ? (payload.metadata as Record<string, unknown>)
      : {};
    const { referenceIds, firstFrameId, lastFrameId } = collectMediaIds(payload, {
    // Only video tasks use first/last frame slots. Named start/end frames win
    // over promoting the first connected image.
    promoteFirstImageToFrame: isVideo,
  });
  const modelValue = String(input.model || config.model || '');
  const model = modelValue ? modelOptionName(modelValue) : undefined;
  const durationRaw = Number(config.videoSeconds ?? metadata.durationSecs ?? 5);
  let duration_secs = Number.isFinite(durationRaw) ? Math.max(1, Math.round(durationRaw)) : 5;
  const rawResolution = String(config.vquality || metadata.resolution || '720p');
  // Always emit model-canonical tokens (`720p` / `768P` / `2K`). Canvas UI may
  // store bare heights (`1080`) or MiniMax values that earlier helpers mangled.
  let resolution = model
    ? canonicalizeVideoResolution(model, rawResolution)
    : canonicalizeVideoResolution('', rawResolution);
  let aspect_ratio = String(config.size || metadata.aspectRatio || '16:9');
  if (model && isMiniMaxH3VideoModel(model)) {
    const hasMedia =
      Boolean(firstFrameId || lastFrameId) || referenceIds.length > 0;
    duration_secs = normalizeMiniMaxH3Duration(duration_secs);
    aspect_ratio = normalizeMiniMaxH3Ratio(aspect_ratio, hasMedia);
  }

  return {
    mode,
    prompt: String(payload.prompt || input.prompt || ''),
    model,
    aspect_ratio,
    resolution,
    duration_secs,
    reference_media_ids: referenceIds,
    first_frame_media_id: firstFrameId,
    last_frame_media_id: lastFrameId,
    ...(input.projectId?.trim() ? { project_id: input.projectId.trim() } : {}),
  };
}

export function createAgentSession(_input: CreateSessionInput): Promise<AgentSessionDetail> {
  return Promise.reject(new Error('Agent sessions are not available in allo canvas'));
}

export function queryAgentSession(_id: string): Promise<AgentSessionDetail> {
  return Promise.reject(new Error('Agent sessions are not available in allo canvas'));
}

export function agentSessionFailureMessage(detail: AgentSessionDetail, fallback = 'Agent session failed') {
  for (let index = detail.tasks.length - 1; index >= 0; index -= 1) {
    const task = detail.tasks[index];
    if ((task.status === 'failed' || task.status === 'cancelled') && task.error?.trim()) {
      return generationErrorMessage(task.error.trim());
    }
  }
  for (let index = detail.messages.length - 1; index >= 0; index -= 1) {
    const message = detail.messages[index];
    if (message.role === 'assistant' && message.content.trim()) {
      return generationErrorMessage(message.content.trim());
    }
  }
  return fallback;
}

export function downloadSessionResults(_id: string) {
  return Promise.resolve([] as TaskResult[]);
}

export function uploadAgentFile(_sessionId: string, _file: File) {
  return Promise.reject(new Error('Agent file upload is not available'));
}

export async function createGenerationTask(input: CreateTaskInput) {
  const body = alloBodyFromCreateInput(input);
  const view = await alloCreate(body);
  const task = mapAlloTask(view, {
    projectId: input.projectId,
    type: input.type || `canvas_${body.mode}`,
    operation: input.operation,
    model: body.model,
    inputJson: input.input ? JSON.stringify(input.input) : undefined,
  });
  notifyCanvasTaskCreated(task);
  return task;
}

export function listGenerationTasks(limit = 30, options?: { projectId?: string; activeOnly?: boolean }) {
  let tasks = [...taskIndex.values()];
  if (options?.projectId) tasks = tasks.filter((t) => t.projectId === options.projectId);
  if (options?.activeOnly) {
    tasks = tasks.filter((t) => t.status === 'queued' || t.status === 'running');
  }
  return Promise.resolve(tasks.slice(0, limit));
}

export async function queryGenerationTask(id: string, _options?: { signal?: AbortSignal }) {
  const view = await alloGet(id);
  const prev = taskIndex.get(id);
  return mapAlloTask(view, {
    projectId: prev?.projectId,
    type: prev?.type,
    operation: prev?.operation,
    inputJson: prev?.inputJson,
  });
}

export function appendTaskTextDelta(_id: string, _content: string) {
  return Promise.reject(new Error('Text deltas are not available'));
}

export function queryTaskTextReplay(_id: string, _after = 0) {
  return Promise.resolve({ deltas: [], complete: true } as TaskTextReplay);
}

export function retryGenerationTask(id: string) {
  return queryGenerationTask(id);
}

export function queryFailedVideoProviderTask(id: string) {
  return queryGenerationTask(id).then((task) => ({
    task,
    providerStatus: task.status,
    recovered: false,
    billingSettled: true,
  }));
}

export async function cancelGenerationTask(id: string) {
  const view = await alloCancel(id);
  const prev = taskIndex.get(id);
  return mapAlloTask(view, {
    projectId: prev?.projectId,
    type: prev?.type,
    operation: prev?.operation,
    inputJson: prev?.inputJson,
  });
}

export function listTaskLogs(id: string) {
  const task = taskIndex.get(id);
  if (!task) return Promise.resolve([] as TaskLog[]);
  const logs: TaskLog[] = [
    {
      id: `${id}-created`,
      taskId: id,
      level: 'info',
      message: 'Task created',
      createdAt: task.createdAt,
    },
  ];
  if (task.error) {
    logs.push({
      id: `${id}-error`,
      taskId: id,
      level: 'error',
      message: task.error,
      createdAt: task.updatedAt,
    });
  }
  return Promise.resolve(logs);
}

export async function waitForGenerationTask(
  id: string,
  options?: {
    signal?: AbortSignal;
    intervalMs?: number;
    timeoutMs?: number;
    initialTask?: GenerationTask;
    onTaskUpdate?: (task: GenerationTask) => void;
  }
) {
  const startedAt = Date.now();
  const intervalMs = options?.intervalMs || 2000;
  let lastTask = options?.initialTask;
  let lastQueryError: unknown;
  try {
    while (Date.now() - startedAt < (options?.timeoutMs || taskWaitTimeoutMs(lastTask))) {
      if (options?.signal?.aborted) throw new DOMException('Aborted', 'AbortError');
      let task: GenerationTask;
      try {
        task = await queryGenerationTask(id, { signal: options?.signal });
        lastTask = task;
        lastQueryError = undefined;
        options?.onTaskUpdate?.(task);
      } catch (error) {
        lastQueryError = error;
        await delay(intervalMs, options?.signal);
        continue;
      }
      if (task.status === 'succeeded') return task;
      if (task.status === 'failed' || task.status === 'cancelled') {
        throw new Error(
          task.error
            ? generationErrorMessage(task.error)
            : `Task ${task.status === 'cancelled' ? 'cancelled' : 'failed'}`
        );
      }
      await delay(intervalMs, options?.signal);
    }
  } catch (error) {
    if (options?.signal?.aborted) {
      await cancelGenerationTask(id).catch(() => undefined);
      throw new DOMException('Aborted', 'AbortError');
    }
    throw error;
  }
  throw new Error(
    lastQueryError instanceof Error
      ? `Task status sync failed: ${lastQueryError.message}`
      : 'Task timed out, please retry later'
  );
}

function taskWaitTimeoutMs(task?: GenerationTask) {
  const type = task?.type || '';
  if (type.includes('storyboard')) return 13 * 60 * 1000;
  if (type.includes('video')) return 32 * 60 * 1000;
  if (type.includes('image')) return 10 * 60 * 1000;
  if (type.includes('text') || type.includes('audio')) return 12 * 60 * 1000;
  return 10 * 60 * 1000;
}

function delay(ms: number, signal?: AbortSignal) {
  return new Promise<void>((resolve, reject) => {
    const timer = window.setTimeout(resolve, ms);
    signal?.addEventListener(
      'abort',
      () => {
        window.clearTimeout(timer);
        reject(new DOMException('Aborted', 'AbortError'));
      },
      { once: true }
    );
  });
}

function notifyCanvasTaskCreated(task: GenerationTask) {
  if (typeof window === 'undefined' || !task.projectId) return;
  window.dispatchEvent(new CustomEvent('canvas:task-created', { detail: { task } }));
}
