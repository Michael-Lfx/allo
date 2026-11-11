import {expect,test} from 'bun:test';
import {parseConversationId} from '@common/types/ids';
import type {VoiceSnapshot} from '@common/adapter/voiceCompanion';
import {resetVoiceHistory,mergeVoiceTranscript,createLiveVoiceHistory,reconcileLiveVoiceHistory} from './liveVoiceTurn';
import {getEpoch,retainConsumer} from '../conversation/Messages/conversationMessageCoordinator';
import {runConversationResetSingleFlight} from '../conversation/platforms/nomi/resetSingleFlight';
import zh from '@renderer/services/i18n/locales/zh-CN/voice.json';
import en from '@renderer/services/i18n/locales/en-US/voice.json';
import {readFileSync} from 'node:fs';

const id=parseConversationId('0190f5fe-7c00-7a00-8000-000000000929');
const snapshot=(session:string):VoiceSnapshot=>({
  instance_id:'host',seq:1,session_id:session,turn_id:'old',phase:'deciding',
  enabled:true,connection:'connected',user_text:'old question',assistant_text:'old answer',error:null,
  acoustic_ready:true,dialogue_ready:true,playback_ready:true,
  binding:{session_id:session,conversation_id:id,binding_id:'binding',workspace:null},projected_turn_ids:[],
  conversation:[{session_id:session,turn_id:'old',user_text:'old question',assistant_text:'old answer',model_text:null}],
});

for (const kind of ['missing','unbound','wrong-session'] as const) {
  test(`clear waits for ${kind} voice scope before invoking any mutation`,async()=>{
    const current=snapshot('clear-'+kind);
    const unavailable=kind==='missing'?null:{...current,binding:kind==='unbound'?null:{...current.binding!,session_id:'other'}};
    const consumer=retainConsumer(id);const epoch=getEpoch(id);let calls=0;
    try {
      expect(await resetVoiceHistory(unavailable,id,async()=>{calls++;}).then(()=>null,error=>(error as Error).message)).toBe('voice_reset_scope_unavailable');
      expect(calls).toBe(0);expect(getEpoch(id)).toBe(epoch);
      expect(mergeVoiceTranscript(current,id,[])).toHaveLength(2);
      await resetVoiceHistory(current,id,async()=>{calls++;});
      expect(calls).toBe(1);expect(getEpoch(id)).toBe(epoch+1);
      expect(mergeVoiceTranscript(current,id,[])).toEqual([]);
    } finally {consumer.release();}
  });
}

test('required reset fails recoverably before IPC and retries once binding is available',async()=>{
  const consumer=retainConsumer(id);const current=snapshot('required-reset');
  const ref={current:null as Promise<void>|null};let view:VoiceSnapshot|null=null;
  const errors:unknown[]=[];let calls=0;let started=0;let settled=0;let successes=0;
  const reset=()=>runConversationResetSingleFlight({inFlightRef:ref,conversationId:id,
    invokeReset:()=>resetVoiceHistory(view,id,async()=>{calls++;}),
    onStart:()=>{started++;},onSuccess:()=>{successes++;},onError:error=>{errors.push(error);},onSettled:()=>{settled++;}});
  try {
    await reset();expect(calls).toBe(0);expect(successes).toBe(0);expect(errors).toHaveLength(1);expect(ref.current).toBeNull();
    view={...current,binding:null};await reset();expect(calls).toBe(0);expect(settled).toBe(2);
    view=current;const first=reset();expect(reset()).toBe(first);await first;
    expect(calls).toBe(1);expect(started).toBe(3);expect(settled).toBe(3);expect(successes).toBe(1);
    expect(mergeVoiceTranscript(current,id,[])).toEqual([]);
  } finally {consumer.release();}
});

test('a late backend failure leaves the frozen turn and epoch intact',async()=>{
  const current=snapshot('failed-reset');const consumer=retainConsumer(id);const epoch=getEpoch(id);
  try {
    expect(await resetVoiceHistory(current,id,async()=>{throw new Error('reset_failed');}).then(()=>null,error=>(error as Error).message)).toBe('reset_failed');
    expect(getEpoch(id)).toBe(epoch);expect(mergeVoiceTranscript(current,id,[])).toHaveLength(2);
  } finally {consumer.release();}
});

test('success after teardown fences old text but preserves the next utterance',async()=>{
  const current=snapshot('unmounted-reset');let done!:()=>void;
  const consumer=retainConsumer(id);
  const reset=resetVoiceHistory(current,id,()=>new Promise<void>(resolve=>{done=resolve;}));
  consumer.release();done();await reset;
  const next={...current,turn_id:'fresh',conversation:[...current.conversation!,{...current.conversation![0],turn_id:'fresh',user_text:'fresh question',assistant_text:null}]};
  const state=reconcileLiveVoiceHistory(createLiveVoiceHistory(),{snapshot:next,conversationId:id,epoch:getEpoch(id),visibleIds:new Set(),currentSequence:1,latestPage:null}).state;
  expect(mergeVoiceTranscript(next,id,[],state).map(row=>(row.content as {content:string}).content)).toEqual(['fresh question']);
});

test('confirmed idle or a call bound to another conversation can clear without suppressing that call',async()=>{
  let calls=0;const current=snapshot('other-conversation');
  await resetVoiceHistory({...current,session_id:null,binding:null,conversation:[]},id,async()=>{calls++;});
  const otherId=parseConversationId('0190f5fe-7c00-7a00-8000-000000000930');
  await resetVoiceHistory(current,otherId,async()=>{calls++;});
  expect(calls).toBe(2);expect(mergeVoiceTranscript(current,id,[])).toHaveLength(2);
});

test('both composer reset entries use the tested guard and expose translated retry guidance',()=>{
  const source=readFileSync(new URL('../conversation/platforms/nomi/NomiSendBox.tsx',import.meta.url),'utf8');
  expect(source).toContain('await resetVoiceHistory(voiceSnapshot, conversation_id, clearContext)');
  expect(source).toContain('resetVoiceHistory(voiceSnapshot, conversation_id, () => ipcBridge.conversation.reset.invoke(params))');
  expect(source.match(/Message.info\(t\('voice.composer.resetScopeUnavailable'\)\)/g)).toHaveLength(2);
  expect(zh.composer.resetScopeUnavailable).toContain('暂未清理');
  expect(en.composer.resetScopeUnavailable).toContain('Nothing was cleared');
});
