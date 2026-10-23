import {cancelVoiceTask,getVoiceTask,type VoiceSnapshot,type VoiceTaskSnapshot} from '@common/adapter/voiceCompanion';

/** The click owns the displayed command, even if another task starts before its response. */
export async function stopDisplayedVoiceTask(conversationId:string,snapshot:VoiceSnapshot|null):Promise<VoiceTaskSnapshot> {
  const task=snapshot?.task;
  if(!snapshot?.instance_id || !task || task.conversation_id!==conversationId)throw new Error('task_scope_changed');
  const frozen={...task};
  if(frozen.status==='completed'||frozen.status==='failed')return frozen;
  if(frozen.status!=='accepted')return getVoiceTask(frozen.command_id);
  if(!frozen.message_id)throw new Error('task_scope_changed');
  return cancelVoiceTask(snapshot.instance_id,frozen);
}
