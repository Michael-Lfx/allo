import React,{useRef,useState} from 'react';
import {Button} from '@arco-design/web-react';
import {useTranslation} from 'react-i18next';
import {openVoiceTask,replayVoiceResult,type VoiceSnapshot} from '@common/adapter/voiceCompanion';

const stages={waiting:'voice.progress.waiting',thinking:'voice.progress.thinking',answering:'voice.progress.answering',tool:'voice.progress.tool',planning:'voice.progress.planning',approval:'voice.progress.approval'} as const;
const health={fresh:'voice.progress.fresh',unverified:'voice.progress.unverified',no_recent_progress:'voice.progress.no_recent_progress'} as const;
const notifications={pending:'voice.notification.pending',dispatched:'voice.notification.dispatched',started:'voice.notification.started',played:'voice.notification.played',failed:'voice.notification.failed',unknown:'voice.notification.unknown',suppressed:'voice.notification.suppressed'} as const;
export default function VoiceTaskDiagnostics({snapshot}:{snapshot:VoiceSnapshot|null}){
  const {t}=useTranslation();
  const [busy,setBusy]=useState(false);
  const [error,setError]=useState<string|null>(null);
  const scope=snapshot?.instance_id+':'+snapshot?.session_id+':'+snapshot?.turn_id;
  const current=useRef(scope);current.current=scope;
  const progress=snapshot?.task_progress;
  const notification=snapshot?.task_notification;
  return <section className='flex flex-col gap-2 text-12px text-t-secondary'>
    {snapshot?.binding&&<div className='flex flex-col gap-1'>
      <Button onClick={()=>{void openVoiceTask(snapshot.binding!.conversation_id).catch(()=>setError(scope));}}>{t('voice.target.bound')}</Button>
      {snapshot.binding.workspace&&<p className='m-0 break-all'>{t('voice.target.workspace')}: {snapshot.binding.workspace}</p>}
    </div>}
    {progress&&progress.command_id===snapshot?.task?.command_id&&<div role='status'>
      <p className='m-0'>{t(stages[progress.stage as keyof typeof stages]??stages.waiting)}</p>
      <p className='m-0'>{t(health[progress.health as keyof typeof health]??health.unverified)}</p>
    </div>}
    {notification&&<p role='status' className='m-0'>{t('voice.notification.title')}: {t(notifications[notification.status as keyof typeof notifications]??notifications.unknown)}</p>}
    {snapshot?.task&&['completed','failed'].includes(snapshot.task.status)&&<Button disabled={busy||!snapshot.enabled||snapshot.connection!=='connected'||snapshot.ended_reason==='user_stop'||!snapshot.session_id||!snapshot.turn_id||['dispatched','started'].includes(notification?.status??'')} onClick={()=>{
      const requestScope=scope;setBusy(true);setError(null);void replayVoiceResult(snapshot,crypto.randomUUID()).catch(()=>{if(current.current===requestScope)setError(requestScope);}).finally(()=>setBusy(false));
    }}>{t('voice.notification.replay')}</Button>}
    {error===scope&&<p role='alert' className='m-0'>{t('voice.task.controlUnknown')}</p>}
  </section>;
}
