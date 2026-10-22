import React,{useState} from 'react';
import {Button} from '@arco-design/web-react';
import {useTranslation} from 'react-i18next';
import {openVoiceTask,stopVoiceSession,wakeVoiceSession} from '@common/adapter/voiceCompanion';
import {useVoiceSnapshot} from './useVoiceSnapshot';
import VoiceTaskQueue from './VoiceTaskQueue';

export default function VoiceConversationBar({conversationId}:{conversationId:string}) {
  const {t}=useTranslation();
  const {snapshot,failed}=useVoiceSnapshot();
  const [pending,setPending]=useState(false);
  const [controlFailed,setControlFailed]=useState(false);
  const bound=snapshot?.binding?.conversation_id===conversationId;
  const active=bound&&Boolean(snapshot?.session_id)&&!snapshot?.ended_reason&&snapshot?.phase!=='idle';
  const otherActive=!bound&&Boolean(snapshot?.session_id)&&!snapshot?.ended_reason&&snapshot?.phase!=='idle';
  const items=snapshot?.queue?.filter(item=>item.task.conversation_id===conversationId);
  return <section className='flex flex-col gap-2 mb-2 text-12px text-t-secondary'>
    <div className='flex items-center flex-wrap gap-2'>
      <span role='status'>{active?t('voice.session.phase.'+snapshot!.phase):t('voice.session.title')}</span>
      <Button size='small' disabled={pending||(otherActive&&!snapshot?.binding)||(!active&&!otherActive&&(!snapshot?.acoustic_ready||snapshot.connection!=='connected'))} onClick={()=>{
        setPending(true);setControlFailed(false);
        const action=otherActive?openVoiceTask(snapshot!.binding!.conversation_id):active?stopVoiceSession(snapshot!):wakeVoiceSession();
        void action.catch(()=>setControlFailed(true)).finally(()=>setPending(false));
      }}>{t(otherActive?'voice.entry.openActive':active?'voice.session.stop':'voice.entry.start')}</Button>
      {bound&&snapshot?.binding?.workspace&&<span className='break-all'>{t('voice.target.workspace')}: {snapshot.binding.workspace}</span>}
    </div>
    <VoiceTaskQueue items={items} instanceId={snapshot?.instance_id}/>
    {(failed||controlFailed)&&<p role='alert' className='m-0'>{t('voice.session.unavailable')}</p>}
  </section>;
}
