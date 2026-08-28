/**
 * Video Canvas document model — aligned with open-ai-canvas semantics,
 * rewritten for allo (Arco / httpBridge). Independent of Creative Workshop.
 */

export type CanvasNodeType =
  | 'image'
  | 'text'
  | 'config'
  | 'video'
  | 'audio'
  | 'script'
  | 'frame'
  | 'drawing'
  | 'skill';

export type CanvasNodeStatus = 'idle' | 'success' | 'loading' | 'error';

export type CanvasGenerationMode = 'text' | 'image' | 'video' | 'audio';

export type CanvasVideoEditOperation =
  | 'text_to_video'
  | 'image_to_video'
  | 'extend'
  | 'camera_motion'
  | 'concat';

export type ViewportTransform = {
  x: number;
  y: number;
  k: number;
};

export type CanvasConnection = {
  id: string;
  fromNodeId: string;
  toNodeId: string;
  fromHandleId?: string;
  toHandleId?: string;
};

export type StoryboardRow = {
  id: string;
  shotNumber: number;
  durationSeconds: number;
  plotDescription: string;
  dialogue: string;
  imageGenerationPrompt: string;
  videoMotionPrompt: string;
  imageNodeId?: string;
  videoNodeId?: string;
  status?: CanvasNodeStatus;
  errorDetails?: string;
};

export type StoryboardData = {
  rows: StoryboardRow[];
};

export type CanvasNodeMetadata = {
  content?: string;
  prompt?: string;
  status?: CanvasNodeStatus;
  errorDetails?: string;
  generationMode?: CanvasGenerationMode;
  model?: string;
  size?: string;
  seconds?: string;
  vquality?: string;
  generateAudio?: string;
  videoEditOperation?: CanvasVideoEditOperation;
  videoStartFrameNodeId?: string;
  videoEndFrameNodeId?: string;
  mediaId?: string;
  /** Exclusive Flowy TV cover. Only one image node should have this set. */
  tvCover?: boolean;
  mimeType?: string;
  durationMs?: number;
  taskId?: string;
  taskProgress?: number;
  storyboard?: StoryboardData;
  locked?: boolean;
};

export type CanvasNodeData = {
  id: string;
  type: CanvasNodeType;
  title: string;
  position: { x: number; y: number };
  width: number;
  height: number;
  parentId?: string;
  metadata?: CanvasNodeMetadata;
};

export type CanvasDocument = {
  schema: 1;
  title: string;
  nodes: CanvasNodeData[];
  connections: CanvasConnection[];
  viewport: ViewportTransform;
  backgroundMode: 'lines' | 'blank' | 'grid';
  /**
   * 项目级时间线（client-doc-first）。
   * 无独立 `/timeline` 路由；随 PUT `/projects/{id}/doc` 持久化。
   */
  timeline?: {
    version: 2;
    tracks: Array<{ id: string; kind: string; label: string; order: number; locked?: boolean }>;
    clips: Array<Record<string, unknown>>;
    durationMs: number;
    updatedAt?: string;
  };
  /** High-fidelity Agent→Canvas sidecar (camera tree, voice bible, write-back). */
  alloCreative?: Record<string, unknown>;
};

export const CANVAS_DOC_SCHEMA = 1 as const;

export function createEmptyCanvasDoc(title = ''): CanvasDocument {
  return {
    schema: CANVAS_DOC_SCHEMA,
    title,
    nodes: [],
    connections: [],
    viewport: { x: 0, y: 0, k: 1 },
    backgroundMode: 'lines',
  };
}

export function defaultNodeSize(type: CanvasNodeType): { width: number; height: number } {
  switch (type) {
    case 'config':
      return { width: 320, height: 360 };
    case 'script':
      return { width: 420, height: 320 };
    case 'video':
      return { width: 280, height: 220 };
    case 'image':
      return { width: 240, height: 220 };
    case 'audio':
      return { width: 260, height: 140 };
    case 'text':
      return { width: 240, height: 160 };
    case 'frame':
      return { width: 480, height: 360 };
    default:
      return { width: 220, height: 160 };
  }
}

export function newNodeId(): string {
  if (typeof crypto !== 'undefined' && 'randomUUID' in crypto) {
    return crypto.randomUUID();
  }
  return `n_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 9)}`;
}
