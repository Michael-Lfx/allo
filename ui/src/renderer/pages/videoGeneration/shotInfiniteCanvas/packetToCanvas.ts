import { CanvasNodeType, type CanvasConnection, type CanvasNodeData, type CanvasNodeMetadata, type CanvasNodeStatus } from '@oc/types/canvas';
import type { ShotPacketView, ShotRefRole, ShotRefSlot } from '../types';
import type { StoryboardVideoSlotStatus } from '../storyboardVideoStatus';
import {
  AUDIO_NODE,
  IMAGE_NODE,
  VIDEO_NODE,
  VIDEO_NODE_ID,
  audioNodeId,
  imageNodeId,
  type MiniPoint,
} from '../shotMiniCanvas/layout';
import { seedancePromptToNodeMentions, shotNodeModelValue } from './shotMentions';

export type ShotCanvasRefKind = 'image' | 'audio';

export type ShotCanvasNodeRef =
  | { kind: 'video' }
  | { kind: ShotCanvasRefKind; slot: number };

export function parseShotNodeId(id: string): ShotCanvasNodeRef | null {
  if (id === VIDEO_NODE_ID) return { kind: 'video' };
  const image = /^image-(\d+)$/.exec(id);
  if (image) return { kind: 'image', slot: Number(image[1]) };
  const audio = /^audio-(\d+)$/.exec(id);
  if (audio) return { kind: 'audio', slot: Number(audio[1]) };
  return null;
}

export function shotStorageKey(kind: ShotCanvasRefKind | 'video', path: string): string {
  return `${kind}:shot:${path}`;
}

function slotBound(slot: ShotRefSlot): boolean {
  return Boolean(slot.path && !slot.unbound);
}

function videoStatus(packet: ShotPacketView, generating: boolean, slotStatus?: StoryboardVideoSlotStatus): CanvasNodeStatus {
  if (generating || packet.run_state === 'generating' || slotStatus === 'generating') return 'loading';
  if (packet.run_state === 'failed') return 'error';
  if (packet.run_state === 'ready' || packet.run_state === 'awaiting_review') return 'success';
  if (packet.run_state === 'script_stale' || packet.run_state === 'continuity_stale') return 'success';
  return 'idle';
}

function nodeBox(id: string, positions: Record<string, MiniPoint>, fallback: { w: number; h: number }, fallbackPos: MiniPoint) {
  const saved = positions[id];
  return {
    x: saved?.x ?? fallbackPos.x,
    y: saved?.y ?? fallbackPos.y,
    w: saved?.w && Number.isFinite(saved.w) ? saved.w : fallback.w,
    h: saved?.h && Number.isFinite(saved.h) ? saved.h : fallback.h,
  };
}

export function packetToCanvasGraph(input: {
  packet: ShotPacketView;
  positions: Record<string, MiniPoint>;
  mediaUrls: Record<string, string>;
  imagePrompts: Record<string, string>;
  prompt: string;
  generating: boolean;
  videoPath?: string | null;
  posterPath?: string | null;
  videoStatus?: StoryboardVideoSlotStatus;
  metaPatch?: Record<string, Partial<CanvasNodeMetadata>>;
  titles: {
    video: string;
    image: (slot: ShotRefSlot) => string;
    audio: (slot: ShotRefSlot) => string;
  };
  models?: {
    image?: string | null;
    video?: string | null;
    audio?: string | null;
  };
}): { nodes: CanvasNodeData[]; connections: CanvasConnection[] } {
  const { packet, positions, mediaUrls, imagePrompts, prompt, generating, titles } = input;
  const imageModel = shotNodeModelValue(input.models?.image);
  const videoModel = shotNodeModelValue(input.models?.video);
  const audioModel = shotNodeModelValue(input.models?.audio);
  const composerPrompt = seedancePromptToNodeMentions(prompt, packet);
  const now = new Date().toISOString();
  const nodes: CanvasNodeData[] = [];
  const connections: CanvasConnection[] = [];
  const imageRefs = packet.image_refs ?? [];
  const audioRefs = (packet.audio_refs ?? []).slice(0, 3);

  imageRefs.forEach((slot, index) => {
    const id = imageNodeId(slot.slot);
    const box = nodeBox(id, positions, IMAGE_NODE, { x: 48, y: 48 + index * (IMAGE_NODE.h + 36) });
    const path = slot.path ?? '';
    const url = path ? mediaUrls[path] : '';
    const bound = slotBound(slot);
    const imagePrompt = path ? imagePrompts[path] : '';
    const patch = input.metaPatch?.[id];
    nodes.push({
      id,
      type: CanvasNodeType.Image,
      title: titles.image(slot),
      position: { x: box.x, y: box.y },
      width: box.w,
      height: box.h,
      createdAt: now,
      updatedAt: now,
      metadata: {
        content: bound && url ? url : undefined,
        storageKey: bound && path ? shotStorageKey('image', path) : undefined,
        prompt: imagePrompt || undefined,
        composerContent: imagePrompt || undefined,
        ...(imageModel ? { model: imageModel } : {}),
        status: bound ? 'success' : 'idle',
        alloVimax: {
          kind: 'image',
          slot: slot.slot,
          role: slot.role,
          path: slot.path ?? null,
          unbound: slot.unbound,
          label: slot.label,
        },
        ...patch,
      },
    });
    if (bound) {
      connections.push({
        id: `ref-${id}`,
        fromNodeId: id,
        toNodeId: VIDEO_NODE_ID,
      });
    }
  });

  audioRefs.forEach((slot, index) => {
    const id = audioNodeId(slot.slot);
    const box = nodeBox(id, positions, AUDIO_NODE, {
      x: 48,
      y: 48 + imageRefs.length * (IMAGE_NODE.h + 36) + 24 + index * (AUDIO_NODE.h + 28),
    });
    const path = slot.path ?? '';
    const url = path ? mediaUrls[path] : '';
    const bound = slotBound(slot);
    const patch = input.metaPatch?.[id];
    nodes.push({
      id,
      type: CanvasNodeType.Audio,
      title: titles.audio(slot),
      position: { x: box.x, y: box.y },
      width: box.w,
      height: box.h,
      createdAt: now,
      updatedAt: now,
      metadata: {
        content: bound && url ? url : undefined,
        storageKey: bound && path ? shotStorageKey('audio', path) : undefined,
        ...(audioModel ? { model: audioModel } : {}),
        status: bound ? 'success' : 'idle',
        alloVimax: {
          kind: 'audio',
          slot: slot.slot,
          role: slot.role,
          path: slot.path ?? null,
          unbound: slot.unbound,
          label: slot.label,
        },
        ...patch,
      },
    });
    if (bound) {
      connections.push({
        id: `ref-${id}`,
        fromNodeId: id,
        toNodeId: VIDEO_NODE_ID,
      });
    }
  });

  const videoBox = nodeBox(VIDEO_NODE_ID, positions, VIDEO_NODE, {
    x: 48 + IMAGE_NODE.w + 120,
    y: 56,
  });
  const videoPath = input.videoPath || packet.takes?.find((take) => take.current)?.video_path || '';
  const videoUrl = videoPath ? mediaUrls[videoPath] : '';
  const posterUrl = input.posterPath ? mediaUrls[input.posterPath] : '';
  const currentTake = packet.takes?.find((take) => take.current);
  const status = videoStatus(packet, generating, input.videoStatus);
  const videoPatch = input.metaPatch?.[VIDEO_NODE_ID];
  nodes.push({
    id: VIDEO_NODE_ID,
    type: CanvasNodeType.Video,
    title: titles.video,
    position: { x: videoBox.x, y: videoBox.y },
    width: videoBox.w,
    height: videoBox.h,
    createdAt: now,
    updatedAt: now,
    metadata: {
      content: videoUrl || undefined,
      previewContent: posterUrl || undefined,
      storageKey: videoPath ? shotStorageKey('video', videoPath) : undefined,
      prompt: composerPrompt,
      composerContent: composerPrompt,
      ...(videoModel ? { model: videoModel } : {}),
      seconds: packet.duration_secs != null ? String(packet.duration_secs) : undefined,
      status,
      taskStatus: status === 'loading' ? 'running' : packet.run_state,
      errorDetails: packet.error || undefined,
      versionLabel: currentTake ? `v${currentTake.take}` : packet.take_count > 0 ? `v${packet.current_take ?? packet.take_count}` : undefined,
      versionPrimary: Boolean(currentTake?.current),
      alloVimax: {
        kind: 'video',
        run_state: packet.run_state,
        scene_root: packet.scene_root,
        shot_idx: packet.shot_idx,
        path: videoPath || null,
        take: currentTake?.take ?? packet.current_take ?? null,
      },
      ...videoPatch,
    },
  });

  return { nodes, connections };
}

export function layoutFromNodes(
  nodes: CanvasNodeData[],
  viewport: { x: number; y: number; k: number }
) {
  const layoutNodes: Record<string, MiniPoint> = {};
  for (const node of nodes) {
    layoutNodes[node.id] = {
      x: node.position.x,
      y: node.position.y,
      w: node.width,
      h: node.height,
    };
  }
  return { viewport, nodes: layoutNodes };
}

export function slotHasGenerationPrompt(slot: ShotRefSlot, kind: ShotCanvasRefKind): boolean {
  if (kind !== 'image') return false;
  if (slot.role === 'continuity_last_frame') return false;
  return Boolean(slot.path && !slot.unbound);
}

export function defaultRoleTitle(kind: ShotCanvasRefKind, role: ShotRefRole, slot: ShotRefSlot): string {
  const custom = slot.label.trim();
  const useCustom =
    Boolean(custom) &&
    custom.length <= 8 &&
    !custom.includes('/') &&
    !custom.includes('_') &&
    role !== 'continuity_last_frame';
  if (useCustom) return custom;
  if (kind === 'audio') return '音色';
  switch (role) {
    case 'continuity_last_frame':
      return '尾帧';
    case 'portrait':
      return '定妆';
    case 'environment':
      return '场景';
    case 'prop':
      return '道具';
    default:
      return '参考';
  }
}

export { nodeSize };
