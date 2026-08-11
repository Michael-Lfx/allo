/** Montage product modes under `/video-generation` (Creation Canvas is separate). */
export type VideoGenMode = 'agent' | 'avatar' | 'creation' | 'talking_head';

/** Pipeline stability from `GET /api/montage/pipelines`. */
export type PipelineStability = 'production' | 'beta';

/** Project run status from checkpoint / `GET .../status`. */
export type MontageRunStatus =
  | 'idle'
  | 'running'
  | 'awaiting_human'
  | 'succeeded'
  | 'failed'
  | 'cancelled';

/** Per-stage status on the Backlot board. */
export type StageStatus =
  | 'pending'
  | 'in_progress'
  | 'awaiting_human'
  | 'completed'
  | 'failed';

export type ApprovalDecision = 'approve' | 'reject' | 'send_back';

export type CheckpointPolicy = 'guided' | 'manual_all' | 'auto_noncreative';

export interface ModelSelection {
  chat?: string | null;
  image?: string | null;
  video?: string | null;
}

export interface OutputSettings {
  aspect: string;
  resolution: string;
  fps: number;
  /** Target finished-film duration in seconds (planning budget). */
  target_duration_secs?: number;
}

/** Lightweight pipeline row from `GET /api/montage/pipelines`. */
export interface PipelineSummary {
  name: string;
  version: string;
  description: string;
  category: string;
  stability: PipelineStability;
  mode: VideoGenMode;
  stage_count: number;
  stage_names: string[];
}

export interface CreateProjectBody {
  title: string;
  pipeline: string;
  prompt: string;
  style_playbook?: string;
  checkpoint_policy?: CheckpointPolicy;
  models?: ModelSelection;
  output?: Partial<OutputSettings>;
  budget_credits?: number;
  reference_video_path?: string;
}

/** `project.json` record from create/list/get. */
export interface ProjectRecord {
  id: string;
  title: string;
  pipeline: string;
  mode: VideoGenMode;
  prompt: string;
  style_playbook?: string | null;
  checkpoint_policy: CheckpointPolicy | string;
  models: ModelSelection;
  output: OutputSettings;
  budget_credits: number;
  reference_video_path?: string | null;
  created_at: string;
  updated_at: string;
}

/** List-row enrichment (status is optional — may be filled by client polling). */
export interface ProjectSummary extends ProjectRecord {
  status?: MontageRunStatus | null;
  current_stage?: string | null;
  final_video?: string | null;
}

export interface Checkpoint {
  project_id: string;
  pipeline: string;
  current_stage: string;
  stage_status?: Record<string, StageStatus>;
  started_at: string;
  updated_at: string;
  status: MontageRunStatus;
  awaiting_human_stage?: string | null;
  last_error?: string | null;
  notes?: string[];
}

export interface ProjectDetail {
  record: ProjectRecord;
  checkpoint: Checkpoint | null;
}

export interface RunStatus {
  status: MontageRunStatus | string;
  current_stage: string;
  awaiting_human_stage?: string | null;
  last_error?: string | null;
  is_job_running: boolean;
  /** Relative project path when a finished cut exists, e.g. `renders/final.mp4`. */
  final_video?: string | null;
}

export interface BoardStage {
  name: string;
  status: StageStatus | string;
  human_approval_default: boolean;
  produces: string[];
  revisions: number;
  send_backs: number;
}

export interface MontageEvent {
  at: string;
  project_id: string;
  stage?: string | null;
  kind: string;
  message: string;
  data?: unknown;
}

export interface BoardState {
  project_id: string;
  pipeline: string;
  status: MontageRunStatus | string;
  current_stage: string;
  awaiting_human_stage?: string | null;
  last_error?: string | null;
  stages: BoardStage[];
  notes: string[];
  recent_events: MontageEvent[];
  final_video?: string | null;
  media_clips?: string[];
}

export interface ApprovalRequest {
  stage: string;
  decision: ApprovalDecision;
  note?: string;
  send_back_to?: string;
}

export interface ProviderMenu {
  chat_ready: boolean;
  image_ready: boolean;
  video_ready: boolean;
  tools: Array<{ name: string; available: boolean; reason?: string | null }>;
}

export type MaterializeToCanvasResult = {
  project_id: string;
  title: string;
  montage_project_id: string;
  node_count: number;
  media_count: number;
  scene_count: number;
  shot_count: number;
  warnings: string[];
  reused?: boolean;
};

export type SyncFromCanvasShot = {
  scene_key: string;
  shot_idx: number;
  media_id: string;
  rel_path?: string;
};

export type SyncFromCanvasResult = {
  montage_project_id: string;
  updated_shots: number;
  warnings: string[];
};

/** Artifact fetch result — Montage artifacts are JSON documents by name. */
export interface ArtifactContent {
  kind: 'text' | 'json' | 'url' | 'binary';
  text?: string;
  url?: string;
  mime?: string;
}

/** TV Show publish / plaza status from Flowy cloud. */
export type TvShowStatus = 'pending' | 'published' | 'offline' | 'deleted';

export interface TvShowAuthor {
  id: number;
  name: string;
  avatarUrl?: string | null;
}

export interface TvShowVideo {
  id: number;
  title: string;
  coverUrl: string;
  workflow: string;
  style?: string | null;
  targetDurationSecs?: number | null;
  status: TvShowStatus | string;
  publishedAt?: string | null;
  submittedAt?: string | null;
  updatedAt?: string | null;
  author: TvShowAuthor;
  likeCount?: number;
  viewCount?: number;
  liked?: boolean;
  isMine?: boolean;
  rejectReason?: string | null;
  description?: string | null;
  packageUrl?: string | null;
  packageSizeBytes?: number | null;
  archiveVersion?: number | null;
  clientSessionId?: string | null;
}

export interface TvShowListResult {
  total: number;
  page: number;
  pageSize: number;
  list: TvShowVideo[];
}

export interface TvShowPublishResult {
  id: number;
  clientSessionId: string;
  title: string;
  status: TvShowStatus | string;
  coverUrl: string;
  packageUrl: string;
  workflow: string;
  submittedAt?: string | null;
  publishedAt?: string | null;
  author: TvShowAuthor;
}

export interface TvShowLikeResult {
  id: number;
  liked: boolean;
  likeCount: number;
}
