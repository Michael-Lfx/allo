import React,{useEffect,useRef,useState} from 'react';
import {Button} from '@arco-design/web-react';
import {useTranslation} from 'react-i18next';
import {cancelVoiceTask,type VoiceQueueItem} from '@common/adapter/voiceCompanion';

export default function VoiceTaskQueue({items=[],instanceId,conversationId}:{items?:VoiceQueueItem[];instanceId?:string;conversationId?:string}) {
  const {t}=useTranslation();
  const [pending,setPending]=useState<string|null>(null);
  const [error,setError]=useState<string|null>(null);
  const scopeId=`${instanceId ?? ''}:${conversationId ?? items[0]?.task.conversation_id ?? ''}`;
  const scope=useRef(scopeId);scope.current=scopeId;
  const generation=useRef(0);
  const currentItems=useRef(items);currentItems.current=items;
  useEffect(()=>{generation.current++;setPending(null);setError(null);return()=>{generation.current++;};},[scopeId]);
  useEffect(()=>{if(pending&&!items.some(item=>item.task.command_id===pending))setPending(null);},[items,pending]);
  if(!items.length)return null;
  return <section className='flex flex-col gap-2' aria-label={t('voice.queue.title')}>
    <h2 className='m-0 text-base font-semibold'>{t('voice.queue.title')}</h2>
    <ol className='m-0 pl-5 flex flex-col gap-2'>
      {items.map(item=><li key={item.task.command_id} className='break-words'>
        <div className='flex items-center flex-wrap gap-2'>
          <span>{item.position===0?t('voice.task.status.'+item.task.status):t('voice.queue.position',{position:item.position})}</span>
          {instanceId&&item.task.conversation_id&&item.task.message_id&&<Button size='mini' disabled={pending!==null} onClick={()=>{
            const requestScope=scopeId;const requestGeneration=generation.current;
            setPending(item.task.command_id);setError(null);
            const sameView=()=>scope.current===requestScope && generation.current===requestGeneration;
            const current=()=>sameView()&&currentItems.current.some(next=>next.task.command_id===item.task.command_id&&next.task.message_id===item.task.message_id);
            void cancelVoiceTask(instanceId,item.task).catch(()=>{if(current())setError(`${requestScope}:${item.task.command_id}`);})
              .finally(()=>{if(sameView())setPending(value=>value===item.task.command_id?null:value);});
          }}>{t('voice.task.cancel')}</Button>}
        </div>
        <p className='m-0 whitespace-pre-wrap'>{item.display_text}</p>
        <p className='m-0 text-12px text-t-secondary break-all'>{t('voice.target.workspace')}: {item.workspace}</p>
      </li>)}
    </ol>
    {items.some(item=>error===`${scopeId}:${item.task.command_id}`)&&<p role='alert' className='m-0'>{t('voice.task.controlUnknown')}</p>}
  </section>;
}
