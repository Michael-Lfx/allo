/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/** ViMax workflow kinds — chosen at session create and locked thereafter. */
export type VimaxWorkflow = 'idea2video' | 'script2video' | 'novel2video';

/** Session / pipeline run status from `GET .../status`. */
export type VimaxRunStatus =
  | 'idle'
  | 'planning'
  | 'rendering'
  | 'succeeded'
  | 'failed'
  | 'cancelled';

/** List-row summary from `GET /api/vimax/sessions`. */
export interface SessionSummary {
  id: string;
  title: string;
  workflow: VimaxWorkflow;
  /** Human-readable pipeline stage label (e.g. "storyboard"). */
  stage?: string | null;
  status?: VimaxRunStatus | null;
  /** RFC3339 string or epoch ms — client normalizes. */
  created_at?: string | number | null;
  updated_at?: string | number | null;
}

/** Full session payload from `GET /api/vimax/sessions/:id`. */
export interface VimaxSession extends SessionSummary {
  idea?: string | null;
  script?: string | null;
  novel_text?: string | null;
  user_requirement?: string | null;
  style?: string | null;
  /** Flowy chat / LLM model id used for planning & revise. */
  llm_model?: string | null;
  /** Flowy image model id used during render. */
  image_model?: string | null;
  /** Flowy video model id used during render. */
  video_model?: string | null;
  /** Relative or absolute URL of the finished video when available. */
  final_video?: string | null;
}

export interface CreateSessionBody {
  workflow: VimaxWorkflow;
  title?: string;
}

export interface PlanBody {
  idea?: string;
  script?: string;
  novel_text?: string;
  user_requirement?: string;
  style?: string;
  llm_model?: string;
  image_model?: string;
  video_model?: string;
}

export interface RenderBody {
  llm_model?: string;
  image_model?: string;
  video_model?: string;
}

export interface ReviseBody {
  revision_target: string;
  revision_instruction: string;
}

export interface SessionStatus {
  stage: string;
  message: string;
  /** 0–100 progress percentage when known. */
  progress: number;
  status: VimaxRunStatus;
  error?: string | null;
  final_video?: string | null;
  /** Recent pipeline progress events (newest may be last). */
  events?: Array<{
    stage: string;
    message: string;
    at?: string;
    metadata?: unknown;
  }>;
}

/** Artifact tree node from `GET .../artifacts`. */
export interface ArtifactNode {
  name: string;
  path: string;
  is_dir: boolean;
  children?: ArtifactNode[];
  /** Optional MIME hint when known. */
  mime?: string | null;
  size?: number | null;
}

/**
 * Artifact fetch result — either inline text/JSON or a binary/media URL.
 * Backend may return a string body, a `{ content }` / `{ url }` object, or a
 * relative serve path; the client normalizes these shapes.
 */
export interface ArtifactContent {
  kind: 'text' | 'json' | 'url' | 'binary';
  /** Inline text (or pretty-printed JSON). */
  text?: string;
  /** Absolute URL for media / binary download. */
  url?: string;
  mime?: string;
}
