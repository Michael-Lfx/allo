import type { I18nKey } from '@/renderer/services/i18n/i18n-keys';
import type { CompanionMotion } from '../characters/types';

/** Feature module that produced a pet status. */
export type PetMessageSource =
  | 'conversation'
  | 'cron'
  | 'learning'
  | 'execution'
  | 'ssh'
  | 'robot'
  | 'requirement'
  | 'autowork'
  | 'app';

/** Lifecycle of one correlated job on the pet channel. */
export type PetMessagePhase = 'queued' | 'running' | 'progress' | 'waiting' | 'completed' | 'failed' | 'cancelled';

export interface PetMessage {
  /** Stable correlation id: updates with the same id replace in place. */
  id: string;
  source: PetMessageSource;
  phase: PetMessagePhase;
  /** i18n key under `nomi.petMessage.*`. */
  titleKey: I18nKey;
  titleParams?: Record<string, string | number>;
  /** Optional process snippet already in the operator's language (truncated). */
  detail?: string;
  /** 0–1 when the producer knows a fraction. */
  progress?: number;
  /** Main-window route to open on click. */
  href?: string;
  updatedAt: number;
}

export const PET_MESSAGE_EVENT = 'nomifun://pet-message';

export const TERMINAL_PHASES: ReadonlySet<PetMessagePhase> = new Set(['completed', 'failed', 'cancelled']);

export function isActivePhase(phase: PetMessagePhase): boolean {
  return !TERMINAL_PHASES.has(phase);
}

export function motionForPetPhase(phase: PetMessagePhase | null): CompanionMotion | null {
  if (!phase) return null;
  switch (phase) {
    case 'queued':
    case 'running':
    case 'progress':
      return 'orbit';
    case 'waiting':
      return 'alert';
    case 'completed':
      return 'notify';
    case 'failed':
      return 'exclaim';
    case 'cancelled':
      return 'wink';
    default:
      return null;
  }
}

export function truncateDetail(text: string, max = 80): string {
  const trimmed = text.replace(/\s+/g, ' ').trim();
  if (trimmed.length <= max) return trimmed;
  return `${trimmed.slice(0, max - 1)}…`;
}
