import React, {useEffect, useRef, useState} from 'react';
import {useTranslation} from 'react-i18next';
import {getVoiceTask, openVoiceTask, sendVoiceKeyboardMessage, type VoiceTaskSnapshot} from '@common/adapter/voiceCompanion';
import {useComposerDraftStore} from '@renderer/stores/composerDraftStore';
import type {I18nKey} from '@renderer/services/i18n/i18n-keys';
import {deliverVoicePetText} from './voicePetDelivery';

/** Authoritative task states plus the local pre-receipt "sending" phase. */
export type VoicePetTrackedStatus = 'sending' | VoiceTaskSnapshot['status'];

const DEFAULT_POLL_INTERVAL_MS = 900;
/** Bounded window (~6 minutes) so a lost task cannot poll forever. */
const DEFAULT_MAX_POLL_ATTEMPTS = 400;

export function isVoicePetTaskTerminal(status: VoicePetTrackedStatus): boolean {
  return status === 'completed' || status === 'failed';
}

/**
 * Only the command this composer submitted may drive its status. A task from
 * another conversation is the global queue's state, never this input's.
 */
export function followsVoicePetTask(task: VoiceTaskSnapshot, commandId: string, conversationId: string): boolean {
  // The exact command is the primary identity. A null conversation is only
  // accepted for that command while the host is still binding it.
  return task.command_id === commandId
    && (!task.conversation_id || task.conversation_id === conversationId);
}

export type VoicePetTaskTrackerPorts = {
  getTask: (commandId: string) => Promise<VoiceTaskSnapshot>;
  onStatus: (status: VoicePetTrackedStatus) => void;
  schedule?: (callback: () => void, delayMs: number) => unknown;
  cancelSchedule?: (handle: unknown) => void;
  intervalMs?: number;
  maxAttempts?: number;
};

/**
 * Poll the exact command returned by the send port. Unknown receipts are
 * reported and retried within a bounded window; a terminal status or a foreign
 * conversation stops immediately. Disposing the returned function clears the
 * pending timer and ignores a receipt that is still in flight.
 */
export function trackVoicePetTask(commandId: string, conversationId: string, ports: VoicePetTaskTrackerPorts): () => void {
  const schedule = ports.schedule ?? ((callback: () => void, delayMs: number) => setTimeout(callback, delayMs));
  const cancelSchedule = ports.cancelSchedule ?? ((handle: unknown) => clearTimeout(handle as ReturnType<typeof setTimeout>));
  const intervalMs = ports.intervalMs ?? DEFAULT_POLL_INTERVAL_MS;
  const maxAttempts = ports.maxAttempts ?? DEFAULT_MAX_POLL_ATTEMPTS;
  let stopped = false;
  let attempts = 0;
  let timer: unknown = null;
  const tick = async () => {
    if (stopped) return;
    let status: VoicePetTrackedStatus;
    try {
      const task = await ports.getTask(commandId);
      if (stopped) return;
      if (!followsVoicePetTask(task, commandId, conversationId)) {
        stopped = true;
        return;
      }
      status = task.status;
    } catch {
      if (stopped) return;
      status = 'unknown';
    }
    ports.onStatus(status);
    attempts += 1;
    if (stopped || isVoicePetTaskTerminal(status) || attempts >= maxAttempts) return;
    timer = schedule(() => {
      void tick();
    }, intervalMs);
  };
  void tick();
  return () => {
    stopped = true;
    if (timer !== null) {
      cancelSchedule(timer);
      timer = null;
    }
  };
}

export type VoicePetInputPorts = {
  /** Defaults to the real voice-companion adapter; injectable for tests. */
  send?: (conversationId: string, commandId: string, text: string) => Promise<{task: VoiceTaskSnapshot}>;
  getTask?: (commandId: string) => Promise<VoiceTaskSnapshot>;
  openTask?: (conversationId: string) => Promise<void>;
  pollIntervalMs?: number;
  maxPollAttempts?: number;
};

export type VoicePetInputProps = {
  conversationId: string;
  workspace: string | null;
  ports?: VoicePetInputPorts;
};

/** This surface belongs only to its frozen voice target; the ordinary pet draft is untouched. */
export default function VoicePetInput({conversationId, workspace, ports = {}}: VoicePetInputProps) {
  const {t} = useTranslation();
  const key = `pet-voice:${conversationId}`;
  const deliveryKey = `${key}:delivery`;
  const text = useComposerDraftStore(state => state.drafts[key]?.text ?? '');
  const setText = (value: string) =>
    useComposerDraftStore.getState().setDraft(key, {text: value, files: [], dir: workspace ?? ''});
  const [failed, setFailed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<VoicePetTrackedStatus | null>(null);
  const [tracked, setTracked] = useState<{conversationId: string; commandId: string} | null>(null);
  const inFlight = useRef(false);
  const runId = useRef(0);
  const portsRef = useRef(ports);
  portsRef.current = ports;

  // A frozen conversation change invalidates any send/poll that belonged to the
  // previous target; unmount does the same through the cleanup below.
  useEffect(() => {
    runId.current += 1;
    inFlight.current = false;
    setBusy(false);
    setFailed(false);
    setStatus(null);
    setTracked(null);
  }, [conversationId]);
  useEffect(
    () => () => {
      runId.current += 1;
    },
    [],
  );

  useEffect(() => {
    if (!tracked || tracked.conversationId !== conversationId) return;
    return trackVoicePetTask(tracked.commandId, conversationId, {
      getTask: (commandId) => (portsRef.current.getTask ?? getVoiceTask)(commandId),
      onStatus: (next) => setStatus(next),
      intervalMs: portsRef.current.pollIntervalMs,
      maxAttempts: portsRef.current.maxPollAttempts,
    });
  }, [tracked, conversationId]);

  const send = async () => {
    if (inFlight.current || !text.trim()) return;
    const run = ++runId.current;
    inFlight.current = true;
    setBusy(true);
    setFailed(false);
    setStatus('sending');
    let acceptedCommandId: string | null = null;
    let initialStatus: VoiceTaskSnapshot['status'] | null = null;
    try {
      const delivery = await deliverVoicePetText(sessionStorage, deliveryKey, conversationId, text, {
        getTask: (commandId) => (portsRef.current.getTask ?? getVoiceTask)(commandId),
        send: async (targetConversationId, commandId, messageText) => {
          const response = await (portsRef.current.send ?? sendVoiceKeyboardMessage)(
            targetConversationId,
            commandId,
            messageText,
          );
          acceptedCommandId = response?.task?.command_id || commandId;
          initialStatus = response?.task?.status ?? null;
          return response;
        },
      });
      if (run !== runId.current) return;
      if (!delivery) {
        setStatus(null);
        return;
      }
      if (useComposerDraftStore.getState().drafts[key]?.text.trim() === delivery.input) {
        useComposerDraftStore.getState().clearDraft(key);
      }
      if (initialStatus) setStatus(initialStatus);
      // Track the identity the backend actually minted, never the global feed.
      setTracked({conversationId, commandId: acceptedCommandId ?? delivery.idempotency_key});
    } catch {
      if (run !== runId.current) return;
      setFailed(true);
      setStatus(null);
      setTracked(null);
      // The draft is deliberately preserved so a human can retry the same text.
    } finally {
      if (run === runId.current) {
        inFlight.current = false;
        setBusy(false);
      }
    }
  };

  const statusKey: I18nKey | null =
    status === null
      ? null
      : status === 'sending'
        ? 'voice.task.status.delivering'
        : (`voice.task.status.${status}` as I18nKey);
  const statusText = statusKey ? t(statusKey) : '';
  const label = workspace?.split(/[\\/]/).filter(Boolean).at(-1) ?? t('voice.target.workspace');
  const openTask = ports.openTask ?? openVoiceTask;
  return (
    <div className='nomi-companion-voice-input' data-companion-hit aria-busy={busy || undefined}>
      <button
        type='button'
        className='nomi-companion-voice-input__target'
        onClick={() => {
          void openTask(conversationId).catch(() => setFailed(true));
        }}
      >
        {t('voice.presentation.inputTarget', {workspace: label})}
      </button>
      <div className='flex items-center gap-2'>
        <input
          aria-label={t('voice.session.sendText')}
          value={text}
          disabled={busy}
          aria-busy={busy || undefined}
          onChange={(event) => setText(event.target.value)}
          placeholder={t('voice.presentation.inputPlaceholder')}
          onKeyDown={(event) => {
            if (event.key === 'Enter' && !event.nativeEvent.isComposing) {
              event.preventDefault();
              void send();
            }
          }}
        />
        <button type='button' disabled={busy || !text.trim()} aria-busy={busy || undefined} onClick={() => void send()}>
          {t('nomi.companion.send')}
        </button>
      </div>
      {statusText && (
        <span role='status' aria-live='polite' className='text-12px text-t-secondary'>
          {statusText}
        </span>
      )}
      {failed && <span role='alert'>{t('voice.presentation.sendFailed')}</span>}
    </div>
  );
}
