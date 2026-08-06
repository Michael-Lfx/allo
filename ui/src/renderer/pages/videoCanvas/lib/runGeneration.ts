/**
 * Run image/video generation or local video concat for a canvas node.
 * Cloud generation uses the same Flowy Seedream / Seedance APIs as Agent mode.
 */

import {
  cancelGenerationTask,
  concatCanvasMedia,
  createGenerationTask,
  waitForGenerationTask,
  type GenerationTaskView,
} from '../api';
import { buildGenerationContext, composePrompt } from './generationContext';
import type { CanvasDocument, CanvasNodeData } from '../types';
import { newNodeId } from '../types';

export type RunGenerationArgs = {
  doc: CanvasDocument;
  nodeId: string;
  onProgress?: (task: GenerationTaskView) => void;
};

export type RunGenerationResult = {
  mediaId: string;
  mode: 'image' | 'video';
  updatedNode: CanvasNodeData;
  spawnedNode?: CanvasNodeData;
};

function patchNode(
  node: CanvasNodeData,
  patch: Partial<CanvasNodeData['metadata']> & { title?: string }
): CanvasNodeData {
  const { title, ...meta } = patch;
  return {
    ...node,
    title: title ?? node.title,
    metadata: { ...(node.metadata ?? {}), ...meta },
  };
}

function spawnResultNode(
  source: CanvasNodeData,
  mode: 'image' | 'video',
  mediaId: string,
  prompt: string,
  model?: string
): CanvasNodeData {
  return {
    id: newNodeId(),
    type: mode === 'video' ? 'video' : 'image',
    title: mode === 'video' ? '生成视频' : '生成图片',
    position: { x: source.position.x + source.width + 48, y: source.position.y },
    width: mode === 'video' ? 280 : 240,
    height: 220,
    metadata: {
      status: 'success',
      mediaId,
      mimeType: mode === 'video' ? 'video/mp4' : 'image/png',
      generationMode: mode,
      prompt,
      model,
    },
  };
}

/** Collect upstream video media ids in connection order (for concat). */
export function upstreamVideoMediaIds(
  nodeId: string,
  doc: CanvasDocument
): string[] {
  const ctx = buildGenerationContext(nodeId, doc.nodes, doc.connections);
  const byId = new Map(doc.nodes.map((n) => [n.id, n]));
  const ids: string[] = [];
  for (const nid of ctx.referenceVideoNodeIds) {
    const mediaId = byId.get(nid)?.metadata?.mediaId;
    if (mediaId) ids.push(mediaId);
  }
  // Also include any referenceMediaIds that belong to video nodes not listed
  // (edge cases) — prefer ordered video node list above.
  if (ids.length === 0) {
    for (const mid of ctx.referenceMediaIds) {
      const node = doc.nodes.find((n) => n.metadata?.mediaId === mid && n.type === 'video');
      if (node) ids.push(mid);
    }
  }
  return ids;
}

export async function runNodeGeneration(args: RunGenerationArgs): Promise<RunGenerationResult> {
  const node = args.doc.nodes.find((n) => n.id === args.nodeId);
  if (!node) throw new Error('node not found');

  const meta = node.metadata ?? {};
  const op = meta.videoEditOperation;

  // Local ffmpeg concat — no cloud task.
  if (op === 'concat') {
    const mediaIds = upstreamVideoMediaIds(node.id, args.doc);
    if (mediaIds.length < 2) {
      throw new Error('拼接至少需要连接 2 个已有视频节点');
    }
    const media = await concatCanvasMedia(mediaIds, '拼接成片');
    const spawned = spawnResultNode(node, 'video', media.media_id, 'concat', meta.model);
    spawned.metadata = {
      ...spawned.metadata,
      videoEditOperation: 'concat',
    };
    return {
      mediaId: media.media_id,
      mode: 'video',
      updatedNode: patchNode(node, { status: 'success', taskProgress: 1 }),
      spawnedNode: spawned,
    };
  }

  const ctx = buildGenerationContext(node.id, args.doc.nodes, args.doc.connections);
  const mode: 'image' | 'video' =
    meta.generationMode === 'video' || node.type === 'video' ? 'video' : 'image';

  let prompt = composePrompt(meta.prompt, ctx);
  if (!prompt && node.type === 'config') {
    prompt = meta.prompt?.trim() || '';
  }
  if (!prompt) throw new Error('请填写提示词，或连接上游文本节点');

  const firstFrameId = meta.videoStartFrameNodeId
    ? args.doc.nodes.find((n) => n.id === meta.videoStartFrameNodeId)?.metadata?.mediaId
    : undefined;
  const lastFrameId = meta.videoEndFrameNodeId
    ? args.doc.nodes.find((n) => n.id === meta.videoEndFrameNodeId)?.metadata?.mediaId
    : undefined;

  // 图生视频：上游图片作为参考；若用户选了 image_to_video 且无首帧，用首张参考图当 first_frame。
  let resolvedFirst = firstFrameId;
  if (
    mode === 'video' &&
    !resolvedFirst &&
    (op === 'image_to_video' || ctx.referenceMediaIds.length > 0) &&
    ctx.referenceImageNodeIds.length > 0
  ) {
    const imgNode = args.doc.nodes.find((n) => n.id === ctx.referenceImageNodeIds[0]);
    resolvedFirst = imgNode?.metadata?.mediaId;
  }

  const durationSecs = meta.seconds ? Number(meta.seconds) : undefined;

  const task = await createGenerationTask({
    mode,
    prompt,
    model: meta.model,
    aspect_ratio: meta.size || '16:9',
    resolution: meta.vquality || '720p',
    duration_secs: Number.isFinite(durationSecs) ? durationSecs : 5,
    reference_media_ids:
      resolvedFirst || lastFrameId
        ? ctx.referenceMediaIds.filter((id) => id !== resolvedFirst && id !== lastFrameId)
        : ctx.referenceMediaIds,
    first_frame_media_id: resolvedFirst,
    last_frame_media_id: lastFrameId,
  });
  args.onProgress?.(task);

  const done = await waitForGenerationTask(task.task_id, {
    onProgress: args.onProgress,
  });
  if (done.status === 'canceled') {
    throw new Error('已取消');
  }
  if (done.status !== 'succeeded' || !done.result_media_id) {
    throw new Error(done.error || '生成失败');
  }

  if (node.type === 'config') {
    const spawned = spawnResultNode(
      node,
      mode,
      done.result_media_id,
      prompt,
      meta.model
    );
    return {
      mediaId: done.result_media_id,
      mode,
      updatedNode: patchNode(node, { status: 'success', taskId: done.task_id, taskProgress: 1 }),
      spawnedNode: spawned,
    };
  }

  return {
    mediaId: done.result_media_id,
    mode,
    updatedNode: patchNode(node, {
      status: 'success',
      mediaId: done.result_media_id,
      mimeType: mode === 'video' ? 'video/mp4' : 'image/png',
      taskId: done.task_id,
      taskProgress: 1,
    }),
  };
}

export async function cancelNodeGeneration(taskId: string): Promise<void> {
  await cancelGenerationTask(taskId);
}
