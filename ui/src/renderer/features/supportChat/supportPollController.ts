/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { ICloudImConversation, ICloudImMessage } from '@/common/adapter/ipcBridge';

export type SupportPollVisibility = 'visible' | 'hidden';

export type SupportPollControllerDeps = {
  getConversation: () => Promise<ICloudImConversation>;
  listMessagesAfter: (afterSeq: number) => Promise<ICloudImMessage[]>;
  onUnread: (conversation: ICloudImConversation) => void;
  onMessages: (messages: ICloudImMessage[]) => void;
  onError?: (error: unknown) => void;
  setTimeout: (fn: () => void, ms: number) => unknown;
  clearTimeout: (id: unknown) => void;
};

export type SupportPollController = {
  start: () => void;
  setModalOpen: (open: boolean) => void;
  setVisibility: (visibility: SupportPollVisibility) => void;
  setAfterSeq: (seq: number) => void;
  pollNow: () => void;
  dispose: () => void;
  /** Test helpers */
  isInFlight: () => boolean;
};

const UNREAD_INTERVAL_MS = 15_000;
const MESSAGE_INTERVAL_MS = 3_000;
const HIDDEN_INTERVAL_MS = 60_000;
const BACKOFF_STEPS_MS = [15_000, 30_000, 60_000] as const;

export function createSupportPollController(deps: SupportPollControllerDeps): SupportPollController {
  let disposed = false;
  let inFlight = false;
  let pendingImmediate = false;
  let modalOpen = false;
  let visibility: SupportPollVisibility = 'visible';
  let afterSeq = 0;
  let failureCount = 0;
  let timer: unknown = null;
  let generation = 0;

  function clearTimer() {
    if (timer != null) {
      deps.clearTimeout(timer);
      timer = null;
    }
  }

  function intervalMs(): number {
    if (visibility === 'hidden') return HIDDEN_INTERVAL_MS;
    if (failureCount > 0) {
      const index = Math.min(failureCount - 1, BACKOFF_STEPS_MS.length - 1);
      return BACKOFF_STEPS_MS[index];
    }
    return modalOpen ? MESSAGE_INTERVAL_MS : UNREAD_INTERVAL_MS;
  }

  function schedule() {
    clearTimer();
    if (disposed) return;
    const gen = generation;
    timer = deps.setTimeout(() => {
      if (disposed || gen !== generation) return;
      void tick();
    }, intervalMs());
  }

  async function tick() {
    if (disposed) return;
    if (inFlight) {
      pendingImmediate = true;
      return;
    }

    inFlight = true;
    const gen = generation;
    try {
      if (modalOpen) {
        const messages = await deps.listMessagesAfter(afterSeq);
        if (disposed || gen !== generation) return;
        deps.onMessages(messages);
        for (const message of messages) {
          if (message.seq > afterSeq) afterSeq = message.seq;
        }
      } else {
        const conversation = await deps.getConversation();
        if (disposed || gen !== generation) return;
        deps.onUnread(conversation);
      }
      failureCount = 0;
    } catch (error) {
      if (disposed || gen !== generation) return;
      failureCount += 1;
      deps.onError?.(error);
    } finally {
      inFlight = false;
      if (disposed || gen !== generation) return;
      if (pendingImmediate) {
        pendingImmediate = false;
        void tick();
        return;
      }
      schedule();
    }
  }

  function pollNow() {
    if (disposed || inFlight) return;
    clearTimer();
    void tick();
  }

  return {
    start() {
      if (disposed) return;
      pollNow();
    },
    setModalOpen(open: boolean) {
      if (disposed) return;
      if (modalOpen === open) return;
      modalOpen = open;
      if (inFlight) {
        pendingImmediate = true;
        clearTimer();
        return;
      }
      pollNow();
    },
    setVisibility(next: SupportPollVisibility) {
      if (disposed) return;
      if (visibility === next) return;
      visibility = next;
      if (next === 'visible') {
        if (inFlight) {
          pendingImmediate = true;
          clearTimer();
          return;
        }
        pollNow();
      } else {
        schedule();
      }
    },
    setAfterSeq(seq: number) {
      afterSeq = Math.max(0, seq);
    },
    pollNow,
    dispose() {
      disposed = true;
      generation += 1;
      pendingImmediate = false;
      clearTimer();
    },
    isInFlight() {
      return inFlight;
    },
  };
}
