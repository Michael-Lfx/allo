import {test,expect} from 'bun:test';
import {petMessageFromVoice} from './voice';
import {motionForPetPhase} from './types';
import type {VoiceSnapshot} from '@common/adapter/voiceCompanion';
const snapshot={binding:{session_id:'s',binding_id:'b',conversation_id:'voice-P',workspace:'P',memory_owner_companion_id:'owner-a'},phase:'listening',error:null,ended_reason:null} as VoiceSnapshot;
test('voice chips stay with the frozen owner and link to the actual conversation',()=>{
  expect(petMessageFromVoice(snapshot,'owner-b')).toBeNull();
  const message=petMessageFromVoice(snapshot,'owner-a');
  expect(message?.href).toBe('/conversation/voice-P');
  expect(motionForPetPhase(message!.phase)).toBe('orbit');
  expect(message?.titleKey).toBe('voice.session.phase.listening');
  const waiting=petMessageFromVoice({...snapshot,confirmation:{approval_id:'a'} as any},'owner-a');
  expect(motionForPetPhase(waiting!.phase)).toBe('alert');
  expect(petMessageFromVoice({...snapshot,ended_reason:'user_stop'},'owner-a')?.phase).toBe('cancelled');
  const running={...snapshot,ended_reason:'user_stop',task:{command_id:'a',conversation_id:'voice-P',status:'accepted'}} as VoiceSnapshot;
  expect(petMessageFromVoice(running,'owner-a')?.phase).toBe('running');
  expect(petMessageFromVoice(running,'owner-a')?.titleKey).toBe('voice.task.status.accepted');
});
