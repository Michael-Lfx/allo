import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { CompanionId } from '@/common/types/ids';
import type { CompanionActivity, CompanionMood, CompanionMotion } from '../characters/types';
import { listenPetMessageBroadcast, subscribePetMessages } from './channel';
import { attachPetMessageProjectors } from './project';
import { foldIncomingPetMessage, resolveCompanionMotion } from './motion';
import { isActivePhase, type PetMessage } from './types';

const TERMINAL_HOLD_MS = 4_800;
const MAX_VISIBLE = 3;

export function useCompanionPetFeed(opts: {
  companionId: CompanionId | null;
  mood: CompanionMood;
  activity: CompanionActivity;
  quiet: boolean;
}): {
  messages: PetMessage[];
  motion: CompanionMotion;
  dismiss: (id: string) => void;
} {
  const { companionId, mood, activity, quiet } = opts;
  const [messages, setMessages] = useState<PetMessage[]>([]);
  const timers = useRef(new Map<string, ReturnType<typeof setTimeout>>());

  const clearTimer = (id: string) => {
    const timer = timers.current.get(id);
    if (timer) {
      clearTimeout(timer);
      timers.current.delete(id);
    }
  };

  const ingest = useCallback(
    (incoming: PetMessage) => {
      setMessages((current) => foldIncomingPetMessage(current, incoming, { quiet, limit: MAX_VISIBLE }));
      clearTimer(incoming.id);
      // Quiet hours drop in-flight chips silently; don't hold a celebration toast.
      if (!quiet && !isActivePhase(incoming.phase)) {
        timers.current.set(
          incoming.id,
          setTimeout(() => {
            timers.current.delete(incoming.id);
            setMessages((current) => current.filter((item) => item.id !== incoming.id));
          }, TERMINAL_HOLD_MS)
        );
      }
    },
    [quiet]
  );

  useEffect(() => {
    const stopLocal = subscribePetMessages(ingest);
    const stopBroadcast = listenPetMessageBroadcast(ingest);
    const stopProjectors = attachPetMessageProjectors(companionId);
    return () => {
      stopLocal();
      stopBroadcast();
      stopProjectors();
      for (const timer of timers.current.values()) clearTimeout(timer);
      timers.current.clear();
    };
  }, [companionId, ingest]);

  const dismiss = useCallback((id: string) => {
    clearTimer(id);
    setMessages((current) => current.filter((item) => item.id !== id));
  }, []);

  const motion = useMemo(() => resolveCompanionMotion(mood, activity, messages), [mood, activity, messages]);

  return { messages, motion, dismiss };
}
