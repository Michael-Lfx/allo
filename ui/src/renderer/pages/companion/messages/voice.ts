import type {VoiceSnapshot} from '@common/adapter/voiceCompanion';
import type {PetMessage,PetMessagePhase} from './types';
import type {I18nKey} from '@renderer/services/i18n/i18n-keys';

/** Only the immutable owner of this binding receives its voice animation/chip. */
export function petMessageFromVoice(snapshot:VoiceSnapshot|null,companionId:string|null):PetMessage|null {
  const binding=snapshot?.binding;
  if(!binding||!companionId||binding.memory_owner_companion_id!==companionId)return null;
  const candidate=snapshot.task?.conversation_id===binding.conversation_id?snapshot.task:null;
  const task=candidate&&(!['completed','failed'].includes(candidate.status)||candidate.acoustic_turn_id===snapshot.turn_id)?candidate:null;
  let phase:PetMessagePhase;
  if(task?.status==='failed'||(!task&&snapshot.error))phase='failed';
  else if(task?.status==='completed')phase='completed';
  else if(snapshot.confirmation||task?.status==='unknown')phase='waiting';
  else if(task?.status==='queued')phase='queued';
  else if(task)phase='running';
  else if(snapshot.ended_reason)phase='cancelled';
  else phase='running';
  const titleKey=(task?`voice.task.status.${task.status}`:snapshot.ended_reason?'voice.session.stop':`voice.session.phase.${snapshot.phase}`) as I18nKey;
  return {id:`voice:${binding.binding_id}`,source:'conversation',phase,titleKey,
    href:`/conversation/${binding.conversation_id}`,updatedAt:Date.now()};
}
