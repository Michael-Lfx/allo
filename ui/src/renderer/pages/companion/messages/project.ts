import { ipcBridge } from '@/common';
import type {
  IApiRobotStatus,
  IApiSshStatus,
  IAutoWorkState,
  IConversationTurnCompletedEvent,
  ICronJob,
  ILearningCourseGenerationEvent,
  ILearningLessonGenerationEvent,
  IRequirement,
} from '@/common/adapter/ipcBridge';
import type { CompanionId } from '@/common/types/ids';
import type {
  TAgentExecutionChangedEvent,
  TAgentExecutionLeadThinkingEvent,
} from '@/common/types/agentExecution/agentExecutionEvents';
import type { I18nKey } from '@/renderer/services/i18n/i18n-keys';
import { ingestPetMessage } from './channel';
import {
  conversationChipId,
  petMessageFingerprint,
  petMessageFromResponseStream,
  petMessageFromTurnStarted,
} from './conversationStream';
import { isActivePhase, truncateDetail, type PetMessage, type PetMessagePhase } from './types';

export { petMessageFromResponseStream, petMessageFromTurnStarted, petMessageFingerprint } from './conversationStream';

const petKey = (key: string): I18nKey => key as I18nKey;

export function petMessageFromTurnCompleted(evt: IConversationTurnCompletedEvent): PetMessage | null {
  const failed = evt.state === 'error';
  const cancelled = evt.state === 'stopped';
  return {
    id: conversationChipId(String(evt.conversation_id), evt.turn_id ? String(evt.turn_id) : null),
    source: 'conversation',
    phase: failed ? 'failed' : cancelled ? 'cancelled' : 'completed',
    titleKey: failed
      ? petKey('nomi.petMessage.conversation.failed')
      : cancelled
        ? petKey('nomi.petMessage.conversation.cancelled')
        : petKey('nomi.petMessage.conversation.completed'),
    detail: evt.detail ? truncateDetail(evt.detail) : undefined,
    href: `/conversation/${evt.conversation_id}`,
    updatedAt: Date.now(),
  };
}

export function petMessageFromCronJob(job: ICronJob): PetMessage | null {
  const status = job.state.last_status;
  if (!status) return null;
  const failed = status === 'error';
  const skipped = status === 'skipped' || status === 'missed';
  return {
    id: `cron:${job.cron_job_id}`,
    source: 'cron',
    phase: failed ? 'failed' : skipped ? 'cancelled' : 'completed',
    titleKey: failed
      ? petKey('nomi.petMessage.cron.failed')
      : skipped
        ? petKey('nomi.petMessage.cron.skipped')
        : petKey('nomi.petMessage.cron.completed'),
    titleParams: { name: job.name },
    detail: job.state.last_error ? truncateDetail(job.state.last_error) : undefined,
    href: '/scheduled',
    updatedAt: Date.now(),
  };
}

export function petMessageFromCronExecuted(evt: {
  cron_job_id: string;
  status: 'ok' | 'error' | 'skipped' | 'missed';
  error?: string;
}): PetMessage {
  const failed = evt.status === 'error';
  const skipped = evt.status === 'skipped' || evt.status === 'missed';
  return {
    id: `cron:${evt.cron_job_id}`,
    source: 'cron',
    phase: failed ? 'failed' : skipped ? 'cancelled' : 'completed',
    titleKey: failed
      ? petKey('nomi.petMessage.cron.failed')
      : skipped
        ? petKey('nomi.petMessage.cron.skipped')
        : petKey('nomi.petMessage.cron.completed'),
    titleParams: { name: evt.cron_job_id.slice(0, 8) },
    detail: evt.error ? truncateDetail(evt.error) : undefined,
    href: '/scheduled',
    updatedAt: Date.now(),
  };
}

const learningPhase = (phase: string): PetMessagePhase => {
  if (phase === 'completed') return 'completed';
  if (phase === 'failed') return 'failed';
  if (phase === 'started' || phase === 'queued') return 'queued';
  return 'progress';
};

export function petMessageFromCourseGeneration(evt: ILearningCourseGenerationEvent): PetMessage {
  return {
    id: `learning:course:${evt.course_id ?? 'active'}`,
    source: 'learning',
    phase: learningPhase(evt.phase),
    titleKey: petKey(`nomi.petMessage.learning.course.${evt.phase}`),
    titleParams: {
      title: evt.title ?? '',
      round: evt.round ?? '',
      max: evt.max_rounds ?? '',
    },
    detail: evt.error ? truncateDetail(evt.error) : evt.text ? truncateDetail(evt.text) : undefined,
    progress:
      evt.round != null && evt.max_rounds != null && evt.max_rounds > 0
        ? Math.min(1, evt.round / evt.max_rounds)
        : undefined,
    href: '/learn',
    updatedAt: Date.now(),
  };
}

export function petMessageFromLessonGeneration(evt: ILearningLessonGenerationEvent): PetMessage {
  return {
    id: `learning:lesson:${evt.lesson_id ?? 'active'}`,
    source: 'learning',
    phase: learningPhase(evt.phase),
    titleKey: petKey(`nomi.petMessage.learning.lesson.${evt.phase}`),
    titleParams: { title: evt.title ?? '', round: evt.round ?? '', max: evt.max_rounds ?? '' },
    detail: evt.error ? truncateDetail(evt.error) : evt.text ? truncateDetail(evt.text) : undefined,
    href: '/learn',
    updatedAt: Date.now(),
  };
}

export function petMessageFromAgentExecution(evt: TAgentExecutionChangedEvent): PetMessage | null {
  const kind = evt.change_kind;
  // `status_changed` is a revision bump with no operator-facing fact — skip it
  // so planning / step copy is not clobbered by "status updated".
  if (kind === 'status_changed') return null;
  const phase: PetMessagePhase =
    kind === 'deleted'
      ? 'cancelled'
      : kind === 'decision_requested'
        ? 'waiting'
        : kind === 'decision_answered'
          ? 'completed'
          : 'running';
  return {
    id: `execution:${evt.execution_id}`,
    source: 'execution',
    phase,
    titleKey: petKey(`nomi.petMessage.execution.${kind}`),
    href: '/guid',
    updatedAt: Date.now(),
  };
}

export function petMessageFromLeadThinking(evt: TAgentExecutionLeadThinkingEvent): PetMessage | null {
  if (evt.done) return null;
  return {
    id: `execution:${evt.execution_id}`,
    source: 'execution',
    phase: 'running',
    titleKey: petKey(`nomi.petMessage.execution.${evt.phase}`),
    href: '/guid',
    updatedAt: Date.now(),
  };
}

export function petMessageFromSsh(evt: IApiSshStatus): PetMessage | null {
  if (evt.state === 'idle') return null;
  const phase: PetMessagePhase =
    evt.state === 'connected' || evt.state === 'closed'
      ? 'completed'
      : evt.state === 'dropped'
        ? 'failed'
        : 'running';
  return {
    id: `ssh:${evt.sshHostId}`,
    source: 'ssh',
    phase,
    titleKey: petKey(`nomi.petMessage.ssh.${evt.state}`),
    detail: evt.detail ? truncateDetail(evt.detail) : undefined,
    href: '/settings/ssh-hosts',
    updatedAt: Date.now(),
  };
}

export function petMessageFromRobot(evt: IApiRobotStatus, companionId: CompanionId | null): PetMessage | null {
  if (evt.companion_id != null && companionId != null && evt.companion_id !== companionId) return null;
  if (evt.phase === 'offline' || evt.phase === 'idle') return null;
  return {
    id: `robot:${evt.robot_id}`,
    source: 'robot',
    phase: 'running',
    titleKey: petKey(`nomi.petMessage.robot.${evt.phase}`),
    href: companionId ? `/nomi?companion=${encodeURIComponent(companionId)}&tab=remote` : '/nomi',
    updatedAt: Date.now(),
  };
}

export function petMessageFromRequirement(req: IRequirement): PetMessage | null {
  const phase: PetMessagePhase =
    req.status === 'done'
      ? 'completed'
      : req.status === 'failed'
        ? 'failed'
        : req.status === 'cancelled'
          ? 'cancelled'
          : req.status === 'needs_review'
            ? 'waiting'
            : req.status === 'in_progress'
              ? 'running'
              : 'queued';
  return {
    id: `requirement:${req.requirement_id}`,
    source: 'requirement',
    phase,
    titleKey: petKey(`nomi.petMessage.requirement.${req.status}`),
    titleParams: { title: req.title, no: req.display_no },
    href: '/requirements',
    updatedAt: Date.now(),
  };
}

export function petMessageFromAutoWork(evt: IAutoWorkState): PetMessage | null {
  if (!(evt.running || evt.run_state === 'active')) return null;
  return {
    id: `autowork:${evt.kind}:${evt.target_id}`,
    source: 'autowork',
    phase: 'running',
    titleKey: evt.tag ? petKey('nomi.petMessage.autowork.runningTagged') : petKey('nomi.petMessage.autowork.running'),
    titleParams: evt.tag ? { tag: evt.tag } : undefined,
    href: '/requirements',
    updatedAt: Date.now(),
  };
}

const emit = (message: PetMessage | null): void => {
  if (message) ingestPetMessage(message);
};

const lastFingerprint = new Map<string, string>();

/** Skip identical process frames (token-by-token content, reasoning deltas). */
function emitDistinct(message: PetMessage | null): void {
  if (!message) return;
  const fingerprint = petMessageFingerprint(message);
  if (lastFingerprint.get(message.id) === fingerprint) return;
  lastFingerprint.set(message.id, fingerprint);
  ingestPetMessage(message);
}

/**
 * SSH / robot / autowork go quiet with `null` instead of a terminal envelope.
 * Remember ids we actually surfaced as in-flight so an idle snapshot does not
 * flash "done", while a live chip still clears when the source stops.
 */
export function projectOrClear(live: Set<string>, next: PetMessage | null, idle: PetMessage): PetMessage | null {
  if (next) {
    if (isActivePhase(next.phase)) live.add(next.id);
    else live.delete(next.id);
    return next;
  }
  if (!live.delete(idle.id)) return null;
  return { ...idle, updatedAt: Date.now() };
}

/** Subscribe domain WS events and fold them onto the pet channel. */
export function attachPetMessageProjectors(companionId: CompanionId | null): () => void {
  const companionConversations = new Set<string>();
  const turnByConversation = new Map<string, string>();
  const live = new Set<string>();
  const emitLifecycle = (next: PetMessage | null, idle: PetMessage): void => {
    emitDistinct(projectOrClear(live, next, idle));
  };
  const robotHref = companionId ? `/nomi?companion=${encodeURIComponent(companionId)}&tab=remote` : '/nomi';
  const unsubs = [
    ipcBridge.conversation.turnStarted.on((evt) => {
      if (evt.companion) {
        companionConversations.add(String(evt.conversation_id));
        return;
      }
      turnByConversation.set(String(evt.conversation_id), String(evt.turn_id));
      emitDistinct(petMessageFromTurnStarted(evt));
    }),
    ipcBridge.conversation.responseStream.on((message) => {
      if (message.companion || companionConversations.has(String(message.conversation_id))) return;
      const turnId = message.turn_id ?? turnByConversation.get(String(message.conversation_id));
      emitDistinct(petMessageFromResponseStream(message, turnId));
    }),
    ipcBridge.conversation.turnCompleted.on((evt) => {
      const cid = String(evt.conversation_id);
      if (companionConversations.has(cid)) return;
      turnByConversation.delete(cid);
      const message = petMessageFromTurnCompleted(evt);
      if (message) lastFingerprint.delete(message.id);
      emit(message);
    }),
    ipcBridge.cron.onJobExecuted.on((evt) => emit(petMessageFromCronExecuted(evt))),
    ipcBridge.learning.courseGeneration.on((evt) => emit(petMessageFromCourseGeneration(evt))),
    ipcBridge.learning.lessonGeneration.on((evt) => emit(petMessageFromLessonGeneration(evt))),
    ipcBridge.agentExecution.events.changed.on((evt) => emitDistinct(petMessageFromAgentExecution(evt))),
    ipcBridge.agentExecution.events.leadThinking.on((evt) => emitDistinct(petMessageFromLeadThinking(evt))),
    ipcBridge.ssh.onStatus.on((evt) =>
      emitLifecycle(petMessageFromSsh(evt), {
        id: `ssh:${evt.sshHostId}`,
        source: 'ssh',
        phase: 'completed',
        titleKey: petKey('nomi.petMessage.ssh.idle'),
        href: '/settings/ssh-hosts',
        updatedAt: Date.now(),
      })
    ),
    ipcBridge.robot.onStatus.on((evt) =>
      emitLifecycle(petMessageFromRobot(evt, companionId), {
        id: `robot:${evt.robot_id}`,
        source: 'robot',
        phase: 'completed',
        titleKey: petKey('nomi.petMessage.robot.idle'),
        href: robotHref,
        updatedAt: Date.now(),
      })
    ),
    ipcBridge.requirements.onStatusChanged.on((req) => emit(petMessageFromRequirement(req))),
    ipcBridge.requirements.onAutoWork.on((evt) =>
      emitLifecycle(petMessageFromAutoWork(evt), {
        id: `autowork:${evt.kind}:${evt.target_id}`,
        source: 'autowork',
        phase: 'completed',
        titleKey: petKey('nomi.petMessage.autowork.completed'),
        titleParams: evt.tag ? { tag: evt.tag } : undefined,
        href: '/requirements',
        updatedAt: Date.now(),
      })
    ),
  ];
  return () => {
    for (const stop of unsubs) stop();
  };
}
