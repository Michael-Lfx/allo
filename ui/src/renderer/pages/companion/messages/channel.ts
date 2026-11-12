import { isTauriRuntime } from '@/common/adapter/tauriRuntime';
import { tauriEvents } from '@/common/adapter/tauriEvents';
import { PET_MESSAGE_EVENT, type PetMessage } from './types';

type Listener = (message: PetMessage) => void;

const listeners = new Set<Listener>();

function normalize(message: Omit<PetMessage, 'updatedAt'> & { updatedAt?: number }): PetMessage {
  return {
    ...message,
    updatedAt: message.updatedAt ?? Date.now(),
    id: message.id.trim() || `pet:${message.source}:${message.updatedAt ?? Date.now()}`,
  };
}

/** Same-window subscribers (tests + the companion page in-process). */
export function subscribePetMessages(listener: Listener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function dispatchLocal(message: PetMessage): void {
  for (const listener of listeners) listener(message);
}

/** Same-window ingest. Companion WS projectors use this so N pets do not fan-out. */
export function ingestPetMessage(message: Omit<PetMessage, 'updatedAt'> & { updatedAt?: number }): PetMessage {
  const payload = normalize(message);
  dispatchLocal(payload);
  return payload;
}

/**
 * Publish a status onto the pet channel. Main-window modules call this for
 * UI-only progress that never hits the backend WS. Companion also projects
 * existing WS events onto the same envelope — producers should not dual-fire.
 */
export function publishPetMessage(message: Omit<PetMessage, 'updatedAt'> & { updatedAt?: number }): PetMessage {
  const payload = ingestPetMessage(message);
  if (isTauriRuntime()) {
    void tauriEvents.emit(PET_MESSAGE_EVENT, payload)
      .catch(() => {
        /* best-effort: WS projectors still cover backend-backed work */
      });
  }
  return payload;
}

/** Companion window: listen for Tauri broadcasts from the main webview. */
export function listenPetMessageBroadcast(listener: Listener): () => void {
  if (!isTauriRuntime()) return () => {};
  let disposed = false;
  let unlisten: (() => void) | undefined;
  void tauriEvents.listen<PetMessage>(PET_MESSAGE_EVENT, listener)
    .then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    })
    .catch(() => {});
  return () => {
    disposed = true;
    unlisten?.();
  };
}
