import React from 'react';
import {Button} from '@arco-design/web-react';
import {Voice} from '@icon-park/react';
import {useTranslation} from 'react-i18next';
import type {VoiceSnapshot} from '@common/adapter/voiceCompanion';
import VoiceTaskQueue from './VoiceTaskQueue';
import VoiceTaskStatus from './VoiceTaskStatus';
import VoiceErrorStatus from './VoiceErrorStatus';

/** Conversation-owned controls, inside the composer; acoustics remain in Settings diagnostics. */
export default function VoiceComposerStatus({conversationId,snapshot,failed=false,historyFailed=false,onRetryHistory}: {
  conversationId:string; snapshot:VoiceSnapshot|null; failed?:boolean;
  historyFailed?:boolean; onRetryHistory?:()=>Promise<void>;
}) {
  const {t}=useTranslation();
  const bound=snapshot?.binding?.conversation_id===conversationId && snapshot.binding.session_id===snapshot.session_id;
  const task=snapshot?.task?.conversation_id===conversationId?snapshot.task:null;
  const items=snapshot?.queue?.filter(item=>item.task.conversation_id===conversationId && item.position>0) ?? [];
  const confirmation=bound?snapshot?.confirmation:null;
  const needsControl=task && (Boolean(confirmation || task.error_code || snapshot?.task_error_code) || ['unknown','delivering','cancelling'].includes(task.status));
  const scope=`${conversationId}:${snapshot?.instance_id ?? ''}`;
  return <div className='flex flex-col gap-8px text-12px text-t-secondary' data-testid='voice-composer-status'>
    <div className='flex items-center flex-wrap gap-8px'>
      <span className='inline-flex items-center gap-6px' role='status' aria-live='polite'>
        <Voice theme='outline' size='14' aria-hidden='true'/>
        {t(`voice.session.phase.${bound?snapshot!.phase:'idle'}`)}
      </span>
      {needsControl&&<details key={`${scope}:${task.command_id}:${confirmation?.approval_id ?? ''}`} open={Boolean(confirmation)}>
        <summary className='cursor-pointer'>{t(confirmation?'voice.task.confirmationTitle':'voice.task.status.'+task.status)}</summary>
        <VoiceTaskStatus compact currentConversationId={conversationId} task={task} showResult={false}
          errorCode={snapshot?.task_error_code} instanceId={snapshot?.instance_id} confirmation={confirmation}/>
      </details>}
      {items.length>0&&<details key={scope}>
        <summary className='cursor-pointer'>{t('voice.composer.queued',{count:items.length})}</summary>
        <VoiceTaskQueue items={items} instanceId={snapshot?.instance_id} conversationId={conversationId}/>
      </details>}
    </div>
    {bound&&<VoiceErrorStatus error={snapshot?.error??null}/>}
    {failed&&<p role='alert' className='m-0'>{t('voice.session.unavailable')}</p>}
    {historyFailed&&<div role='alert' className='flex items-center gap-8px'>
      <span>{t('voice.composer.historyFailed')}</span>
      <Button type='text' size='mini' onClick={()=>{void onRetryHistory?.();}}>{t('voice.composer.retryHistory')}</Button>
    </div>}
  </div>;
}
