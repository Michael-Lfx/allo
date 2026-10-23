import type { CompanionActivity, CompanionMood, CompanionMotion } from '../characters/types';
import { isActivePhase, motionForPetPhase, type PetMessage, type PetMessagePhase } from './types';
import type { VoiceSnapshot } from '@common/adapter/voiceCompanion';

/**
 * Temporary voice call posture mapped from active acoustic session phases (CP-12, CP-13).
 * Triggers dedicated character postures (e.g. Bolt listening, thinking, speaking)
 * during live voice call, independent of general task phases.
 */
export function motionFromVoicePhase(
  voice: VoiceSnapshot | null | undefined,
  companionId: string | null | undefined
): CompanionMotion | null {
  if (!voice || !companionId) return null;
  const binding = voice.binding;
  if (!binding) return null;
  const ownerId = binding.memory_owner_companion_id?.trim();
  if (!ownerId || ownerId !== companionId) {
    return null;
  }
  if (!voice.session_id || voice.ended_reason || voice.phase === 'idle') {
    return null;
  }
  switch (voice.phase) {
    case 'listening':
    case 'follow_up':
      return 'alert';
    case 'transcribing':
    case 'deciding':
      return 'thinking';
    case 'synthesizing':
    case 'speaking':
      return 'notify';
    default:
      return null;
  }
}

const PHASE_PRIORITY: Record<PetMessagePhase, number> = {
  waiting: 50,
  running: 40,
  progress: 40,
  queued: 30,
  failed: 20,
  completed: 10,
  cancelled: 5,
};

export function leadingPetPhase(messages: PetMessage[]): PetMessagePhase | null {
  let best: PetMessage | null = null;
  for (const message of messages) {
    if (!best || PHASE_PRIORITY[message.phase] > PHASE_PRIORITY[best.phase]) best = message;
    else if (best && PHASE_PRIORITY[message.phase] === PHASE_PRIORITY[best.phase] && message.updatedAt > best.updatedAt) {
      best = message;
    }
  }
  return best?.phase ?? null;
}

export function motionFromMood(mood: CompanionMood, activity: CompanionActivity): CompanionMotion {
  if (activity === 'thinking') return 'thinking';
  switch (mood) {
    case 'sleepy':
      return 'sleep';
    case 'excited':
      return 'play';
    case 'happy':
      return 'wink';
    case 'worried':
      return 'wide';
    default:
      return 'idle';
  }
}

/** Pet-channel phase wins over idle mood so the figure narrates live work. */
export function resolveCompanionMotion(
  mood: CompanionMood,
  activity: CompanionActivity,
  messages: PetMessage[]
): CompanionMotion {
  const fromPet = motionForPetPhase(leadingPetPhase(messages));
  if (fromPet) return fromPet;
  return motionFromMood(mood, activity);
}

export function mergePetMessage(current: PetMessage[], incoming: PetMessage, limit = 3): PetMessage[] {
  const without = current.filter((item) => item.id !== incoming.id);
  const next = [incoming, ...without].sort((a, b) => {
    const activeDelta = Number(isActivePhase(b.phase)) - Number(isActivePhase(a.phase));
    if (activeDelta !== 0) return activeDelta;
    return b.updatedAt - a.updatedAt;
  });
  return next.slice(0, limit);
}

/**
 * Quiet hours skip celebratory completes, but must still drop an in-flight
 * chip when that same job ends — otherwise the pet stays "working" all night.
 */
export function foldIncomingPetMessage(
  current: PetMessage[],
  incoming: PetMessage,
  opts: { quiet: boolean; limit?: number }
): PetMessage[] {
  if (opts.quiet && (incoming.phase === 'completed' || incoming.phase === 'cancelled')) {
    return current.filter((item) => item.id !== incoming.id);
  }
  return mergePetMessage(current, incoming, opts.limit ?? 3);
}
