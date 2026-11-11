import {useEffect,useState,useSyncExternalStore} from 'react';
import {
  subscribeVoiceSession,
  getPendingVoiceCallConversationId,
  subscribeVoiceCallIntent,
  getActiveVoiceCallConversationId,
  subscribeActiveVoiceCallConversationId,
  type VoiceSnapshot,
} from '@common/adapter/voiceCompanion';

export function usePendingVoiceCallConversationId() {
  return useSyncExternalStore(subscribeVoiceCallIntent,getPendingVoiceCallConversationId,()=>null);
}

export function useActiveVoiceConversationId() {
  return useSyncExternalStore(subscribeActiveVoiceCallConversationId, getActiveVoiceCallConversationId, () => null);
}

/** View-only subscription. Mount/reconnect never starts capture or resends input. */
export function useVoiceSnapshot(enabled=true) {
  const [snapshot,setSnapshot]=useState<VoiceSnapshot|null>(null);
  const [failed,setFailed]=useState(false);
  useEffect(()=>{
    if(!enabled){setSnapshot(null);return;}
    let disposed=false;
    let off:(()=>void)|undefined;
    void subscribeVoiceSession(next=>{if(!disposed)setSnapshot(next);},undefined,code=>{if(!disposed)setFailed(Boolean(code));})
      .then(unsubscribe=>{if(disposed)unsubscribe();else off=unsubscribe;})
      .catch(()=>{if(!disposed)setFailed(true);});
    return()=>{disposed=true;off?.();};
  },[enabled]);
  return {snapshot,failed};
}
