import {expect,test} from 'bun:test';
import {liveVoiceTurn} from './liveVoiceTurn';
import type {VoiceSnapshot} from '@common/adapter/voiceCompanion';

const snapshot={phase:'deciding',session_id:'session',turn_id:'b',binding:{session_id:'session',binding_id:'binding',conversation_id:'voice',workspace:'D:/owned'},
  conversation:[{turn_id:'a',user_text:'已经落盘',assistant_text:'完成',model_text:null},{turn_id:'b',user_text:'即时转写',assistant_text:'第一句',reply_state:'streaming',model_text:'raw private JSON'}],projected_turn_ids:['a']} as VoiceSnapshot;
test('ASR and first sentence remain visible before durable completion under the same turn identity',()=>{
  const live=liveVoiceTurn(snapshot,'voice')!;
  expect(live.conversation!.map(turn=>turn.turn_id)).toEqual(['b']);
  expect(live.user_text).toBe('即时转写');
  expect(live.assistant_text).toBe('第一句');
  expect(liveVoiceTurn(snapshot,'another-conversation')).toBeNull();
  expect(liveVoiceTurn({...snapshot,session_id:'replacement-session'},'voice')).toBeNull();
});
test('durable receipts retire live placeholders; volume changes cannot reopen them',()=>{
  const completed={...snapshot,projected_turn_ids:['a','b']};
  expect(liveVoiceTurn(completed,'voice')).toBeNull();
  expect(liveVoiceTurn({...completed,seq:99,audio:{rms_milli:42} as VoiceSnapshot['audio']},'voice')).toBeNull();
});
test('ignored cleanup leaves no transcript while the connection continues listening',()=>{
  expect(liveVoiceTurn({...snapshot,phase:'listening',conversation:[],user_text:null,assistant_text:null},'voice')).toBeNull();
});
