import type { IConversationTurnStartedEvent, IResponseMessage } from '@/common/adapter/ipcBridge';
import { compactDisplayText, toDisplayText } from '@/common/chat/displayText';
import { normalizeToolGroupStatus } from '@/common/chat/toolGroupStatus';
import { toolPreparingHintFromEvent } from '@/common/chat/toolPreparing';
import type { MessageId } from '@/common/types/ids';
import type { I18nKey } from '@/renderer/services/i18n/i18n-keys';
import { truncateDetail, type PetMessage, type PetMessagePhase } from './types';

const petKey = (key: string): I18nKey => key as I18nKey;

const GENERIC_THOUGHT = new Set(['thinking', 'thought', 'reasoning', '思考', '推理', '']);

const TOOL_TARGET_FIELDS = ['file_path', 'filePath', 'path', 'command', 'cmd', 'query', 'pattern', 'url', 'glob', 'skill'];

export function conversationChipId(conversationId: string, turnId?: string | null): string {
  return `conversation:${conversationId}:${turnId || 'turn'}`;
}

export function petMessageFingerprint(message: PetMessage): string {
  const params = message.titleParams
    ? Object.keys(message.titleParams)
        .sort()
        .map((key) => `${key}=${message.titleParams![key]}`)
        .join('&')
    : '';
  return `${message.id}|${message.phase}|${message.titleKey}|${message.detail ?? ''}|${params}|${message.progress ?? ''}`;
}

function turnPhaseKey(phase: string | undefined, state: string): I18nKey {
  if (state === 'error') return petKey('nomi.petMessage.conversation.failed');
  if (state === 'stopped') return petKey('nomi.petMessage.conversation.cancelled');
  if (state === 'ai_waiting_confirmation' || phase === 'waiting_permission') {
    return petKey('nomi.petMessage.conversation.waiting');
  }
  switch (phase) {
    case 'starting':
    case 'initializing':
    case 'preparing':
      return petKey('nomi.petMessage.conversation.preparing');
    case 'thinking':
      return petKey('nomi.petMessage.conversation.thinking');
    case 'streaming':
    case 'running':
      return petKey('nomi.petMessage.conversation.streaming');
    case 'tooling':
      return petKey('nomi.petMessage.conversation.tooling');
    default:
      if (state === 'initializing') return petKey('nomi.petMessage.conversation.preparing');
      if (state === 'ai_generating') return petKey('nomi.petMessage.conversation.streaming');
      return petKey('nomi.petMessage.conversation.running');
  }
}

function turnEnvelopePhase(evt: { status: string; state: string; phase?: string }): PetMessagePhase {
  if (evt.state === 'error') return 'failed';
  if (evt.state === 'stopped') return 'cancelled';
  if (evt.status === 'finished') return 'completed';
  if (evt.state === 'ai_waiting_confirmation' || evt.phase === 'waiting_permission') return 'waiting';
  if (evt.status === 'pending' || evt.phase === 'starting' || evt.phase === 'preparing' || evt.state === 'initializing') {
    return 'queued';
  }
  return 'running';
}

export function petMessageFromTurnStarted(evt: IConversationTurnStartedEvent): PetMessage | null {
  if (evt.companion) return null;
  return {
    id: conversationChipId(String(evt.conversation_id), String(evt.turn_id)),
    source: 'conversation',
    phase: turnEnvelopePhase(evt),
    titleKey: turnPhaseKey(evt.phase, evt.state),
    detail: evt.detail ? truncateDetail(evt.detail) : undefined,
    href: `/conversation/${evt.conversation_id}`,
    updatedAt: Date.now(),
  };
}

function thoughtTopic(data: unknown): string | undefined {
  if (!data || typeof data !== 'object' || Array.isArray(data)) return undefined;
  const subject = compactDisplayText((data as { subject?: unknown }).subject).trim();
  if (!subject || GENERIC_THOUGHT.has(subject.toLowerCase())) return undefined;
  return truncateDetail(subject, 32);
}

function compactToolTarget(value: unknown): string | undefined {
  if (value == null) return undefined;
  if (typeof value === 'string') {
    const trimmed = value.trim();
    if (!trimmed) return undefined;
    if (trimmed.startsWith('{')) {
      try {
        return compactToolTarget(JSON.parse(trimmed));
      } catch {
        return truncateDetail(trimmed.replace(/\s+/g, ' '), 40);
      }
    }
    return truncateDetail(trimmed.replace(/\s+/g, ' '), 40);
  }
  if (typeof value === 'object' && !Array.isArray(value)) {
    const record = value as Record<string, unknown>;
    for (const field of TOOL_TARGET_FIELDS) {
      const hit = record[field];
      if (typeof hit === 'string' && hit.trim()) return truncateDetail(hit.trim(), 40);
    }
  }
  const compact = compactDisplayText(value).trim();
  return compact ? truncateDetail(compact, 40) : undefined;
}

function activeToolHint(data: unknown): { mode: 'confirming' | 'executing'; name: string; target?: string } | null {
  if (!Array.isArray(data)) return null;
  const tools = data.flatMap((item) => {
    if (!item || typeof item !== 'object' || Array.isArray(item)) return [];
    const tool = item as Record<string, unknown>;
    const status = normalizeToolGroupStatus(tool.status);
    if (status !== 'Confirming' && status !== 'Executing') return [];
    const name = compactDisplayText(tool.name).trim();
    const target = compactToolTarget(tool.description) ?? compactToolTarget(tool.input) ?? compactToolTarget(tool.params);
    return [{ mode: status === 'Confirming' ? ('confirming' as const) : ('executing' as const), name: name || 'Tool', target }];
  });
  return tools.find((tool) => tool.mode === 'confirming') ?? tools.find((tool) => tool.mode === 'executing') ?? null;
}

function conversationBase(message: IResponseMessage, turnId: string): Pick<PetMessage, 'id' | 'source' | 'href' | 'updatedAt'> {
  return {
    id: conversationChipId(String(message.conversation_id), turnId),
    source: 'conversation',
    href: `/conversation/${message.conversation_id}`,
    updatedAt: Date.now(),
  };
}

/**
 * Live process frames on `message.stream`. `turn.started` is admission-only
 * (`phase: starting`); the specific thinking / tool / write states live here.
 */
export function petMessageFromResponseStream(message: IResponseMessage, turnId?: MessageId | string | null): PetMessage | null {
  if (message.companion) return null;
  if (message.hidden) return null;
  if (message.stream_complete) return null;
  const resolvedTurn = turnId ? String(turnId) : message.turn_id ? String(message.turn_id) : 'turn';
  const base = conversationBase(message, resolvedTurn);

  switch (message.type) {
    case 'start':
      return { ...base, phase: 'queued', titleKey: petKey('nomi.petMessage.conversation.preparing') };
    case 'thought':
    case 'thinking': {
      const topic = thoughtTopic(message.data);
      return {
        ...base,
        phase: 'running',
        titleKey: topic ? petKey('nomi.petMessage.conversation.thinkingOn') : petKey('nomi.petMessage.conversation.thinking'),
        titleParams: topic ? { topic } : undefined,
      };
    }
    case 'tool_preparing': {
      const hint = toolPreparingHintFromEvent(message.data);
      if (!hint) return { ...base, phase: 'running', titleKey: petKey('nomi.petMessage.conversation.tooling') };
      const target = hint.target?.value ? truncateDetail(hint.target.value, 40) : undefined;
      return {
        ...base,
        phase: 'running',
        titleKey: target ? petKey('nomi.petMessage.conversation.callingOn') : petKey('nomi.petMessage.conversation.calling'),
        titleParams: target ? { tool: hint.tool, target } : { tool: hint.tool },
      };
    }
    case 'tool_group': {
      const tool = activeToolHint(message.data);
      if (!tool) return null;
      const action = [tool.name, tool.target].filter(Boolean).join(' · ');
      if (tool.mode === 'confirming') {
        return {
          ...base,
          phase: 'waiting',
          titleKey: petKey('nomi.petMessage.conversation.waitingOn'),
          titleParams: { action },
        };
      }
      return {
        ...base,
        phase: 'running',
        titleKey: tool.target
          ? petKey('nomi.petMessage.conversation.callingOn')
          : petKey('nomi.petMessage.conversation.calling'),
        titleParams: tool.target ? { tool: tool.name, target: tool.target } : { tool: tool.name },
      };
    }
    case 'permission':
    case 'acp_permission':
      return { ...base, phase: 'waiting', titleKey: petKey('nomi.petMessage.conversation.waiting') };
    case 'moa_progress': {
      const record = message.data && typeof message.data === 'object' ? (message.data as { done?: unknown; total?: unknown }) : {};
      const done = typeof record.done === 'number' ? record.done : 0;
      const total = typeof record.total === 'number' ? record.total : 0;
      return {
        ...base,
        phase: 'progress',
        titleKey: petKey('nomi.petMessage.conversation.moa'),
        titleParams: { done, total },
        progress: total > 0 ? Math.min(1, done / total) : undefined,
      };
    }
    case 'content':
    case 'text':
      return { ...base, phase: 'running', titleKey: petKey('nomi.petMessage.conversation.streaming') };
    case 'finish':
      return { ...base, phase: 'progress', titleKey: petKey('nomi.petMessage.conversation.finalizing') };
    case 'error':
      return {
        ...base,
        phase: 'failed',
        titleKey: petKey('nomi.petMessage.conversation.failed'),
        detail: typeof message.data === 'string' ? truncateDetail(toDisplayText(message.data)) : undefined,
      };
    default:
      return null;
  }
}
