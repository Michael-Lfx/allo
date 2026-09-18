import type { ArtifactNode, SessionStatus, VimaxRunStatus } from '../types';
import type { StudioStageVariant } from '../studioStageTimeline';

export type StudioSessionRole = 'user' | 'assistant' | 'error' | 'system';

export type StudioSessionMessageKind =
  | 'user_brief'
  | 'user_note'
  | 'milestone'
  | 'gate_render'
  | 'gate_action'
  | 'film_ready'
  | 'failure'
  | 'cancelled';

export type StudioNarrativeBeat =
  | 'plan'
  | 'storyboard'
  | 'portraits'
  | 'world'
  | 'render_frames'
  | 'render_clips'
  | 'film'
  | 'action_assets'
  | 'action_generate';

export type StudioMediaKind = 'image' | 'video' | 'audio' | 'file' | 'document';

export type StudioDocumentRole = 'story' | 'script' | 'cast';

export interface StudioSessionMedia {
  id: string;
  kind: StudioMediaKind;
  path: string;
  label?: string;
  /** Matches `StoryboardScene.id` when this card is a shot. */
  sceneId?: string;
  /** Cameo previews load via the Cameo file API, not the artifact tree. */
  origin?: 'artifact' | 'cameo';
  /** Narrative document shown on the planning beat. */
  role?: StudioDocumentRole;
  /** Extra files to merge into this document (e.g. per-scene scripts). */
  paths?: string[];
}

export interface StudioSessionMessage {
  id: string;
  role: StudioSessionRole;
  kind: StudioSessionMessageKind;
  beat?: StudioNarrativeBeat;
  stage?: string;
  live?: boolean;
  pollWaitSecs?: number | null;
  media?: StudioSessionMedia[];
  at?: string;
  text?: string;
  error?: string;
}

export type StudioComposerAction = 'plan' | 'render' | 'continue' | 'stop' | 'none';

export interface StudioUserNote {
  id: string;
  text: string;
}

export interface ProjectStudioSessionInput {
  sourceText?: string;
  status?: SessionStatus | null;
  artifacts: ArtifactNode[];
  hasStoryboard: boolean;
  hasFinalVideo: boolean;
  coverPath?: string | null;
  finalVideoPath?: string | null;
  isAction: boolean;
  actionAssetsReady?: boolean;
  notes?: StudioUserNote[];
  variant?: StudioStageVariant;
  runStatus?: VimaxRunStatus | null;
  /** Home-uploaded Cameo stills / script files shown on the user brief bubble. */
  briefMedia?: StudioSessionMedia[];
}

export interface StudioComposerActionInput {
  busy: boolean;
  isFailed: boolean;
  isAction: boolean;
  hasStoryboard: boolean;
  hasFinalVideo: boolean;
  actionAssetsReady: boolean;
  canRender: boolean;
}
