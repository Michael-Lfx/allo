import React,{useEffect,useRef,useState} from 'react';
import {Button} from '@arco-design/web-react';
import {useTranslation} from 'react-i18next';
import {cancelVoiceTask,type VoiceQueueItem} from '@common/adapter/voiceCompanion';

export default function VoiceTaskQueue({items=[],instanceId}:{items?:VoiceQueueItem[];instanceId?:string}) {
  const {t}=useTranslation();
  const [pending,setPending]=useState<string|null>(null);
  const [error,setError]=useState<string|null>(null);
  const scope=useRef(instanceId);scope.current=instanceId;
  useEffect(()=>{setPending(null);setError(null);},[instanceId]);
  if(!items.length)return null;
  return <section className='flex flex-col gap-2' aria-label={t('voice.queue.title')}>
    <h2 className='m-0 text-base font-semibold'>{t('voice.queue.title')}</h2>
    <ol className='m-0 pl-5 flex flex-col gap-2'>
      {items.map(item=><li key={item.task.command_id} className='break-words'>
        <div className='flex items-center flex-wrap gap-2'>
          <span>{item.position===0?t('voice.task.status.'+item.task.status):t('voice.queue.position',{position:item.position})}</span>
          {instanceId&&item.task.conversation_id&&item.task.message_id&&<Button size='mini' disabled={pending!==null} onClick={()=>{
            const requestInstance=instanceId;setPending(item.task.command_id);setError(null);
            void cancelVoiceTask(instanceId,item.task).catch(()=>{if(scope.current===requestInstance)setError(requestInstance);})
              .finally(()=>{if(scope.current===requestInstance)setPending(null);});
          }}>{t('voice.task.cancel')}</Button>}
        </div>
        <p className='m-0 whitespace-pre-wrap'>{item.display_text}</p>
        <p className='m-0 text-12px text-t-secondary break-all'>{t('voice.target.workspace')}: {item.workspace}</p>
      </li>)}
    </ol>
    {error===instanceId&&<p role='alert' className='m-0'>{t('voice.task.controlUnknown')}</p>}
  </section>;
}
