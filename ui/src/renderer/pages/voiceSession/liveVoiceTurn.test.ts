import {expect, test} from 'bun:test';
import {liveVoiceTurn, buildVoiceLiveMessages, mergeVoiceTranscript, createLiveVoiceHistory, reconcileLiveVoiceHistory, prepareVoiceHistoryReset} from './liveVoiceTurn';
import type {VoiceSnapshot} from '@common/adapter/voiceCompanion';
import {parseConversationId} from '@common/types/ids';
import type {TMessage} from '@common/chat/chatLib';
import {retainConsumer,getEpoch} from '../conversation/Messages/conversationMessageCoordinator';
const id = parseConversationId('0190f5fe-7c00-7a00-8000-000000000002');
const turn:NonNullable<VoiceSnapshot['conversation']>[number] = {session_id:'session',turn_id:'b',user_text:'即时转写',assistant_text:'第一句',reply_state:'streaming',model_text:'private JSON',user_message_id:'u',assistant_message_id:'a'};
const snapshot = {instance_id:'host',phase:'deciding',session_id:'session',turn_id:'b',binding:{session_id:'session',binding_id:'binding',conversation_id:id,workspace:'D:/owned'},conversation:[turn],projected_turn_ids:[]} as unknown as VoiceSnapshot;
const rows=(...ids:string[])=>ids.map(message_id=>({message_id})) as TMessage[];
const content=(messages:TMessage[])=>messages.map(message=>(message.content as {content:string}).content);
const reconcile=(previous=createLiveVoiceHistory(),patch:Partial<Parameters<typeof reconcileLiveVoiceHistory>[1]>={})=>reconcileLiveVoiceHistory(previous,{snapshot,conversationId:id,epoch:0,visibleIds:new Set(),currentSequence:1,latestPage:{conversationId:id,epoch:0,sequence:1},...patch});
test('ASR and streamed reply use ordinary rows; router JSON stays private',()=>{
 expect(content(buildVoiceLiveMessages(snapshot,id,[]))).toEqual(['即时转写','第一句']);
 expect(liveVoiceTurn(snapshot,'other')).toBeNull();
 expect(liveVoiceTurn({...snapshot,session_id:'new'},id)).toBeNull();
});
test('durable rows retire each side independently, not on receipt alone',()=>{
 const projected={...snapshot,projected_turn_ids:['b']};
 expect(content(buildVoiceLiveMessages(projected,id,[]))).toHaveLength(2);
 expect(content(buildVoiceLiveMessages(projected,id,rows('u')))).toEqual(['第一句']);
 expect(buildVoiceLiveMessages(projected,id,rows('u','a'))).toEqual([]);
});
test('receipt before GET plus next utterance keeps the old reply until post-receipt success',()=>{
 const projected={...snapshot,projected_turn_ids:['b']};
 const observed=reconcile(reconcile().state,{snapshot:projected});
 expect(observed.refreshKey).not.toBe('');
 const next={...projected,turn_id:'c',conversation:[turn,{...turn,turn_id:'c',user_message_id:null,assistant_message_id:null,user_text:'下一句',assistant_text:null}]} as VoiceSnapshot;
 const pending=reconcile(observed.state,{snapshot:next,currentSequence:2});
 expect(content(buildVoiceLiveMessages(next,id,[],pending.state))).toEqual(['即时转写','第一句','下一句']);
 const fresh=reconcile(pending.state,{snapshot:next,currentSequence:2,latestPage:{conversationId:id,epoch:0,sequence:2}});
 expect(content(buildVoiceLiveMessages(next,id,[],fresh.state))).toEqual(['下一句']);
});
test('failed/stale GET retains text and a successful retry retires it',()=>{
 const projected={...snapshot,projected_turn_ids:['b']};
 const observed=reconcile(reconcile().state,{snapshot:projected,currentSequence:5});
 const failed=reconcile(observed.state,{snapshot:projected,currentSequence:6,latestPage:{conversationId:id,epoch:0,sequence:4}});
 expect(content(buildVoiceLiveMessages(projected,id,[],failed.state))).toHaveLength(2);
 expect(failed.refreshKey).toBe(observed.refreshKey);
 const retry=reconcile(failed.state,{snapshot:projected,currentSequence:7,latestPage:{conversationId:id,epoch:0,sequence:7}});
 expect(buildVoiceLiveMessages(projected,id,[],retry.state)).toEqual([]);
});
test('paged-out retired rows never return on volume changes, memory stays bounded',()=>{
 const loaded=reconcile(undefined,{visibleIds:new Set(['u','a'])});
 const jitter={...snapshot,seq:99,audio:{rms_milli:88}} as VoiceSnapshot;
 const paged=reconcile(loaded.state,{snapshot:jitter});
 expect(buildVoiceLiveMessages(jitter,id,[],paged.state)).toEqual([]);
 const advanced=reconcile(paged.state,{snapshot:{...snapshot,conversation:[]}});
 expect(advanced.state.retiredIds.size).toBe(0);expect(advanced.state.receipts.size).toBe(0);
});
test('missing or foreign original session cannot enter a new workspace timeline',()=>{
 const polluted={...snapshot,conversation:[{...turn,session_id:undefined},{...turn,turn_id:'old',session_id:'old'}]};
 expect(buildVoiceLiveMessages(polluted,id,[])).toEqual([]);
 expect(buildVoiceLiveMessages(snapshot,parseConversationId('0190f5fe-7c00-7a00-8000-000000000001'),[])).toEqual([]);
});
test('global task A never suppresses legitimate speech in active B',()=>{
 const busy={...snapshot,task:{command_id:'A',conversation_id:'other',status:'accepted'}} as VoiceSnapshot;
 expect(content(buildVoiceLiveMessages(busy,id,[]))).toEqual(['即时转写','第一句']);
});
test('synthetic notice without provenance is excluded, ID prefixes do not classify real turns',()=>{
 expect(buildVoiceLiveMessages({...snapshot,conversation:[{turn_id:'notice',assistant_text:'foreign',user_text:null,model_text:null}]},id,[])).toEqual([]);
 expect(content(buildVoiceLiveMessages({...snapshot,conversation:[{...turn,turn_id:'task-result-valid'}]},id,[]))).toHaveLength(2);
});
test('ignore removes temporary input while listening continues',()=>{
 expect(liveVoiceTurn({...snapshot,phase:'listening',conversation:[],user_text:null,assistant_text:null},id)).toBeNull();
});
test('reset epoch blocks late text from the cleared turn, next turn is accepted',()=>{
 const before=reconcile();const reset=reconcile(before.state,{epoch:1});
 expect(buildVoiceLiveMessages(snapshot,id,[],reset.state)).toEqual([]);
 const late={...snapshot,conversation:[{...turn,assistant_text:'迟到的旧回答'}]};
 const after=reconcile(reset.state,{snapshot:late,epoch:1});
 expect(buildVoiceLiveMessages(late,id,[],after.state)).toEqual([]);
 const next={...snapshot,turn_id:'new',conversation:[turn,{...turn,turn_id:'new',user_message_id:null,assistant_message_id:null}]} as VoiceSnapshot;
 const advanced=reconcile(after.state,{snapshot:next,epoch:1});
 expect(content(buildVoiceLiveMessages(next,id,[],advanced.state))).toEqual(['即时转写','第一句']);
});
test('new instance and independent chat cannot inherit retired identities',()=>{
 const retired=reconcile(undefined,{visibleIds:new Set(['u','a'])});
 const replacement={...snapshot,instance_id:'host-2'};
 const fresh=reconcile(retired.state,{snapshot:replacement});
 expect(content(buildVoiceLiveMessages(replacement,id,[],fresh.state))).toHaveLength(2);
 expect(content(buildVoiceLiveMessages(snapshot,id,[],createLiveVoiceHistory()))).toHaveLength(2);
});

test('a later durable question cannot move the previous unfinished reply to the transcript tail',()=>{
 const next={...turn,turn_id:'c',user_message_id:'u-c',assistant_message_id:'a-c',user_text:'下一问',assistant_text:'下一答'};
 const snap={...snapshot,turn_id:'c',conversation:[turn,next]};
 const persisted=rows('u','u-c','a-c');
 const merged=mergeVoiceTranscript(snap,id,persisted);
 expect(merged.map(row=>row.message_id ?? (row.content as {content:string}).content)).toEqual(['u','第一句','u-c','a-c']);
 const assistantFirst=mergeVoiceTranscript(snapshot,id,rows('a'));
 expect(assistantFirst.map(row=>row.message_id ?? (row.content as {content:string}).content)).toEqual(['即时转写','a']);
});

test('remount does not append old projected rows before its post-receipt page completes',()=>{
 const projected={...snapshot,projected_turn_ids:['b']};
 const remounted=reconcile(undefined,{snapshot:projected});
 expect(remounted.refreshKey).not.toBe('');
 expect(mergeVoiceTranscript(projected,id,[],remounted.state)).toEqual([]);
 const fetched=reconcile(remounted.state,{snapshot:projected,currentSequence:2,latestPage:{conversationId:id,epoch:0,sequence:2}});
 expect(mergeVoiceTranscript(projected,id,[],fetched.state)).toEqual([]);
 expect(fetched.refreshKey).toBe('');
});

test('a cleared unprojected turn stays invalid after the last history consumer leaves and remounts',()=>{
 const consumer=retainConsumer(id);
 const before=reconcile(undefined,{epoch:getEpoch(id)});
 prepareVoiceHistoryReset(snapshot,id)();
 const cleared=reconcile(before.state,{epoch:getEpoch(id)});
 expect(mergeVoiceTranscript(snapshot,id,[],cleared.state)).toEqual([]);
 consumer.release();
 const reopened=retainConsumer(id);
 try {
  const remounted=reconcile(createLiveVoiceHistory(),{epoch:getEpoch(id)});
  expect(mergeVoiceTranscript(snapshot,id,[],remounted.state)).toEqual([]);
 } finally {reopened.release();}
});

test('a successful clear survives unavailable snapshots, while a new turn and another session stay usable',()=>{
 const current={...snapshot,session_id:'clear-with-gap',binding:{...snapshot.binding!,session_id:'clear-with-gap'},conversation:[{...turn,session_id:'clear-with-gap'}]};
 prepareVoiceHistoryReset(current,id)();
 const absent=reconcile(undefined,{snapshot:null});
 const returned=reconcile(absent.state,{snapshot:current});
 expect(mergeVoiceTranscript(current,id,[],returned.state)).toEqual([]);
 const next={...current,turn_id:'next',conversation:[...current.conversation,{...turn,turn_id:'next',session_id:'clear-with-gap',user_message_id:null,assistant_message_id:null}]};
 expect(content(mergeVoiceTranscript(next,id,[]))).toEqual(['即时转写','第一句']);
 const other={...current,session_id:'different',binding:{...current.binding,session_id:'different'},conversation:[{...turn,session_id:'different'}]};
 expect(mergeVoiceTranscript(other,id,[])).toHaveLength(2);
});

test('a failed reset does not invalidate text and an older reset response cannot replace a newer call fence',()=>{
 const current={...snapshot,session_id:'clear-order',binding:{...snapshot.binding!,session_id:'clear-order'},conversation:[{...turn,session_id:'clear-order'}]};
 const older=prepareVoiceHistoryReset(snapshot,id);
 const commit=prepareVoiceHistoryReset(current,id);
 expect(mergeVoiceTranscript(current,id,[])).toHaveLength(2); // Not committed: backend failed.
 commit();older();
 expect(mergeVoiceTranscript(current,id,[])).toEqual([]);
 const late={...current,conversation:[{...current.conversation[0],assistant_text:'late old response'}]};
 expect(mergeVoiceTranscript(late,id,[])).toEqual([]);
});

test('a fresh utterance arriving with the first post-reset epoch is not mistaken for cleared speech',()=>{
 const current={...snapshot,session_id:'reset-race',binding:{...snapshot.binding!,session_id:'reset-race'},conversation:[{...turn,session_id:'reset-race'}]};
 const before=reconcile(undefined,{snapshot:current});
 prepareVoiceHistoryReset(current,id)();
 const next={...current,turn_id:'fresh',conversation:[...current.conversation,{...turn,session_id:'reset-race',turn_id:'fresh',user_text:'fresh question',assistant_text:null,user_message_id:null,assistant_message_id:null}]};
 const after=reconcile(before.state,{snapshot:next,epoch:1});
 expect(content(mergeVoiceTranscript(next,id,[],after.state))).toEqual(['fresh question']);
});

const persisted=():TMessage[]=>[
 {message_id:'u-task',msg_id:'u-task',content:{content:'任务进度？'},position:'right'} as TMessage,
 {message_id:'a-task',msg_id:'a-task',content:{content:'查询任务状态'},position:'left'} as TMessage,
];
const answered={...snapshot,conversation:[{...turn,turn_id:'b',user_message_id:'u-task',assistant_message_id:'a-task',user_text:'任务进度？',assistant_text:'任务已完成，结果 42。'}]} as VoiceSnapshot;
test('a foreign binding cannot rewrite another conversation’s persisted result',()=>{
 const foreign={...answered,binding:{...answered.binding!,conversation_id:parseConversationId('0190f5fe-7c00-7a00-8000-000000000001')}};
 expect(content(mergeVoiceTranscript(foreign,id,persisted()))).toEqual(['任务进度？','查询任务状态']);
});
test('a blocked turn cannot rewrite a persisted result',()=>{
 expect(content(mergeVoiceTranscript(answered,id,persisted(),{blockedTurns:new Set(['b'])}))).toEqual(['任务进度？','查询任务状态']);
});
test('a cleared instance or late turn cannot rewrite persisted history',()=>{
 const current={...answered,session_id:'clear-result',binding:{...answered.binding!,session_id:'clear-result'},conversation:[{...answered.conversation![0],session_id:'clear-result'}]};
 prepareVoiceHistoryReset(current,id)();
 const late={...current,conversation:[{...current.conversation![0],assistant_text:'迟到的假结果'}]};
 expect(content(mergeVoiceTranscript(late,id,persisted()))).toEqual(['任务进度？','查询任务状态']);
 const oldInstance={...late,instance_id:'old-host'};
 expect(content(mergeVoiceTranscript(oldInstance,id,persisted()))).toEqual(['任务进度？','查询任务状态']);
});
test('a missing snapshot leaves durable results untouched and same text with a new id stays distinct',()=>{
 expect(content(mergeVoiceTranscript(null,id,persisted()))).toEqual(['任务进度？','查询任务状态']);
 const sameTextDifferentId={...answered,conversation:[{...answered.conversation![0],assistant_message_id:'a-live',assistant_text:'查询任务状态'}]} as VoiceSnapshot;
 expect(mergeVoiceTranscript(sameTextDifferentId,id,persisted())).toHaveLength(3);
});
test('a stale instance cannot add live rows when the current instance is known',()=>{
 const state={...createLiveVoiceHistory(),instanceId:'host'};
 const old={...snapshot,instance_id:'old-host'};
 expect(mergeVoiceTranscript(old,id,[],state)).toEqual([]);
 expect(content(mergeVoiceTranscript(snapshot,id,[],state))).toEqual(['即时转写','第一句']);
});
