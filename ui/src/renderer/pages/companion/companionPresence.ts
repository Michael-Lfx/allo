import type { CompanionActivity } from '@/common/protocolBindings/CompanionActivity';
import type { CompanionHitArea } from '@/common/protocolBindings/CompanionHitArea';
import type { CompanionMood } from '@/common/protocolBindings/CompanionMood';
import type { HitIntent } from '@/common/protocolBindings/HitIntent';
import type { PresenceAgentSnapshot } from '@/common/protocolBindings/PresenceAgentSnapshot';
import type { PresenceState } from '@/common/protocolBindings/PresenceState';

const MOODS: readonly CompanionMood[] = ['happy', 'content', 'sleepy', 'worried', 'excited'];
const ACTIVITIES: readonly CompanionActivity[] = [
  'idle',
  'thinking',
  'busy',
  'awaiting_user',
  'review',
  'failed',
  'interacting',
];

export const DEFAULT_HIT_AREAS: CompanionHitArea[] = [
  { id: 'head', intent: 'head', x: 0.22, y: 0.02, w: 0.56, h: 0.3 },
  { id: 'body', intent: 'body', x: 0.16, y: 0.32, w: 0.68, h: 0.62 },
];

export function asCompanionMood(raw: string | null | undefined): CompanionMood {
  const value = (raw ?? '').trim().toLowerCase();
  return MOODS.find((mood) => mood === value) ?? 'content';
}

export function asCompanionActivity(raw: string | null | undefined): CompanionActivity {
  const value = (raw ?? '').trim().toLowerCase();
  return ACTIVITIES.find((activity) => activity === value) ?? 'idle';
}

export function hitIntentAt(
  areas: CompanionHitArea[],
  x01: number,
  y01: number,
): HitIntent | null {
  for (const area of areas) {
    if (x01 >= area.x && x01 <= area.x + area.w && y01 >= area.y && y01 <= area.y + area.h) {
      return area.intent;
    }
  }
  return null;
}

export function pointInFigure(
  clientX: number,
  clientY: number,
  rect: { left: number; top: number; width: number; height: number },
): { x01: number; y01: number } | null {
  if (rect.width <= 0 || rect.height <= 0) return null;
  return {
    x01: (clientX - rect.left) / rect.width,
    y01: (clientY - rect.top) / rect.height,
  };
}

/** Same priority as Rust `compose_activity`, for overlay-local stream hints. */
export function composeOverlayActivity(input: {
  interacting: boolean;
  learnRunning: boolean;
  localTurnRunning: boolean;
  statusActivity?: CompanionActivity | null;
  snapshot?: PresenceAgentSnapshot | null;
}): CompanionActivity {
  if (input.interacting) return 'interacting';
  if (input.learnRunning) return 'thinking';
  if (input.localTurnRunning) return 'busy';
  if (input.statusActivity) return input.statusActivity;
  const snapshot = input.snapshot;
  if (!snapshot) return 'idle';
  if (snapshot.learn_running) return 'thinking';
  const execution = snapshot.execution_status;
  if (execution === 'awaiting_approval' || execution === 'waiting_input') return 'awaiting_user';
  if (execution === 'planning' || execution === 'paused') return 'review';
  if (execution === 'failed' || execution === 'completed_with_failures') return 'failed';
  if (execution === 'running' || execution === 'starting') return 'busy';
  if (snapshot.pending_confirmations || snapshot.runtime_state === 'waiting_confirmation') {
    return 'awaiting_user';
  }
  if (
    snapshot.conversation_processing ||
    snapshot.summoned ||
    snapshot.runtime_state === 'running' ||
    snapshot.runtime_state === 'starting'
  ) {
    return 'busy';
  }
  return 'idle';
}

export function presenceFromStatus(status: {
  mood?: string | null;
  activity?: string | null;
  stats?: PresenceState['stats'] | null;
  active_clip?: string | null;
  interaction?: PresenceState['interaction'] | null;
  hit_areas?: CompanionHitArea[] | null;
}): {
  mood: CompanionMood;
  activity: CompanionActivity;
  hitAreas: CompanionHitArea[];
} {
  return {
    mood: asCompanionMood(status.mood),
    activity: asCompanionActivity(status.activity),
    hitAreas: status.hit_areas && status.hit_areas.length > 0 ? status.hit_areas : DEFAULT_HIT_AREAS,
  };
}
