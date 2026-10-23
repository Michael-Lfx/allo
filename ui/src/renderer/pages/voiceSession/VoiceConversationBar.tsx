import React, {useRef, useState} from 'react';
import {Button} from '@arco-design/web-react';
import {useTranslation} from 'react-i18next';
import {openVoiceTask, stopVoiceSession} from '@common/adapter/voiceCompanion';
import {useVoiceSnapshot} from './useVoiceSnapshot';
import VoiceTaskQueue from './VoiceTaskQueue';
import VoiceTaskStatus from './VoiceTaskStatus';
import VoiceTaskDiagnostics from './VoiceTaskDiagnostics';
import VoiceAudioStatus from './VoiceAudioStatus';
import VoiceErrorStatus from './VoiceErrorStatus';
import type {VoiceSnapshot} from '@common/adapter/voiceCompanion';

export default function VoiceConversationBar({conversationId,showHangup=false}:{conversationId:string;showHangup?:boolean}) {
  const {snapshot,failed}=useVoiceSnapshot();
  return <VoiceConversationStatus conversationId={conversationId} showHangup={showHangup} snapshot={snapshot} failed={failed}/>;
}

export function VoiceConversationStatus({conversationId,showHangup=false,snapshot,failed=false}:{conversationId:string;showHangup?:boolean;snapshot:VoiceSnapshot|null;failed?:boolean}) {
  const {t}=useTranslation();
  const [pending,setPending]=useState<string|null>(null);
  const [controlFailed,setControlFailed]=useState<string|null>(null);
  const scope=useRef(conversationId);scope.current=conversationId;
  const bound=snapshot?.binding?.conversation_id===conversationId;
  const active=Boolean(snapshot?.session_id)&&!snapshot?.ended_reason&&snapshot?.phase!=='idle';
  const items=snapshot?.queue?.filter(item=>item.task.conversation_id===conversationId && item.position>0);
  const task=snapshot?.task?.conversation_id===conversationId?snapshot.task:null;
  const activeTask=task&&!['completed','failed'].includes(task.status);
  const workspace=bound?snapshot?.binding?.workspace:null;
  const folder=workspace?.split(/[\\/]/).filter(Boolean).at(-1);
  const workspaceName=folder&&/^[0-9a-f]{8}-[0-9a-f-]{27}$/i.test(folder)?t('voice.target.workspace'):folder;
  if(!active&&!activeTask&&!items?.length&&!snapshot?.error&&!failed&&!controlFailed)return null;
  return <section className='flex flex-col gap-2 mb-2 text-12px text-t-secondary' aria-label={t('voice.session.title')}>
    <div className='flex items-center flex-wrap gap-2'>
      <span role='status' aria-live='polite'>{bound&&active?t('voice.session.phase.'+snapshot!.phase):t('voice.session.title')}</span>
      {workspace&&<span className='truncate max-w-48' title={workspace}>{workspaceName}</span>}
      {active&&(!bound||showHangup)&&<Button size='small' status={bound?'danger':undefined} disabled={pending===conversationId||!snapshot?.binding} onClick={()=>{
        const requested=conversationId;setPending(requested);setControlFailed(null);
        const action=!bound?openVoiceTask(snapshot!.binding!.conversation_id):stopVoiceSession(snapshot!);
        void action.catch(()=>{if(scope.current===requested)setControlFailed(requested);}).finally(()=>{if(scope.current===requested)setPending(null);});
      }}>{t(!bound?'voice.entry.openActive':'voice.session.hangup')}</Button>}
      {bound&&<details><summary className='cursor-pointer'>{t('voice.queue.diagnostics')}</summary>
        <VoiceAudioStatus snapshot={snapshot} compact/>
        <VoiceTaskDiagnostics snapshot={snapshot}/>
        {snapshot?.conversation?.filter(turn=>turn.understood_request).map(turn=><details key={'understood-'+turn.turn_id}><summary>{t('voice.session.understoodRequest')}</summary><p className='whitespace-pre-wrap break-words m-0'>{turn.understood_request}</p></details>)}
        {snapshot?.conversation?.filter(turn=>turn.model_text).map(turn=><details key={turn.turn_id}><summary>{t('voice.session.modelText')}</summary><pre className='whitespace-pre-wrap break-words'>{turn.model_text}</pre></details>)}
      </details>}
    </div>
    {activeTask&&<VoiceTaskStatus compact currentConversationId={conversationId} task={task} showResult={false} errorCode={snapshot?.task_error_code} instanceId={snapshot?.instance_id} confirmation={bound?snapshot?.confirmation:null}/>}
    <VoiceTaskQueue items={items} instanceId={snapshot?.instance_id}/>
    {bound&&<VoiceErrorStatus error={snapshot?.error??null}/>}
    {(failed||controlFailed===conversationId)&&<p role='alert' className='m-0'>{t('voice.session.unavailable')}</p>}
  </section>;
}
