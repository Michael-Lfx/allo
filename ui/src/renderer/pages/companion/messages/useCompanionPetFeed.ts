import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { CompanionId } from '@/common/types/ids';
import type { CompanionActivity, CompanionMood, CompanionMotion } from '../characters/types';
import { listenPetMessageBroadcast, subscribePetMessages } from './channel';
import { attachPetMessageProjectors } from './project';
import { foldIncomingPetMessage, resolveCompanionMotion } from './motion';
import { isActivePhase, type PetMessage } from './types';
import type {VoiceSnapshot} from '@common/adapter/voiceCompanion';
import {petMessageFromVoice} from './voice';

const TERMINAL_HOLD_MS = 4_800;
const MAX_VISIBLE = 3;

export function useCompanionPetFeed(opts: {
  companionId: CompanionId | null;
  mood: CompanionMood;
  activity: CompanionActivity;
  quiet: boolean;
  voice?: VoiceSnapshot|null;
}): {
  messages: PetMessage[];
  motion: CompanionMotion;
  dismiss: (id: string) => void;
} {
  const { companionId, mood, activity, quiet } = opts;
  const voiceRef=useRef(opts.voice);voiceRef.current=opts.voice;
  const voiceConversations=useRef(new Set<string>());
  if(opts.voice?.binding)voiceConversations.current.add(opts.voice.binding.conversation_id);
  const voiceFingerprint=useRef('');
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
    const stopProjectors = attachPetMessageProjectors(companionId,id=>voiceConversations.current.has(id));
    return () => {
      stopLocal();
      stopBroadcast();
      stopProjectors();
      for (const timer of timers.current.values()) clearTimeout(timer);
      timers.current.clear();
    };
  }, [companionId, ingest]);

  useEffect(()=>{
    const binding=voiceRef.current?.binding;
    const id=binding?.memory_owner_companion_id===companionId?`voice:${binding?.binding_id}`:null;
    setMessages(current=>current.filter(item=>!item.id.startsWith('voice:')||item.id===id));
    for(const key of timers.current.keys())if(key.startsWith('voice:')&&key!==id)clearTimer(key);
    voiceFingerprint.current='';
  },[opts.voice?.binding?.binding_id,companionId]);

  useEffect(()=>{
    const message=petMessageFromVoice(opts.voice??null,companionId);
    if(!message)return;
    const fingerprint=`${message.id}:${message.phase}:${message.titleKey}`;
    if(voiceFingerprint.current===fingerprint)return;
    voiceFingerprint.current=fingerprint;
    ingest(message);
  },[opts.voice,companionId,ingest]);

  const dismiss = useCallback((id: string) => {
    clearTimer(id);
    setMessages((current) => current.filter((item) => item.id !== id));
  }, []);

  const motion = useMemo(() => resolveCompanionMotion(mood, activity, messages), [mood, activity, messages]);

  return { messages, motion, dismiss };
}
