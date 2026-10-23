import React, { useEffect, useRef, useState } from 'react';
import { Button } from '@arco-design/web-react';
import { useTranslation } from 'react-i18next';
import { cancelVoiceTask, confirmVoiceTask, getVoiceTask, openVoiceTask, type VoiceTaskSnapshot, type VoiceToolConfirmation } from '@common/adapter/voiceCompanion';

export default function VoiceTaskStatus({ task, errorCode, instanceId, confirmation, showResult = true, currentConversationId, compact = false }: {task:VoiceTaskSnapshot|null|undefined;errorCode:string|null|undefined;instanceId?:string;confirmation?:VoiceToolConfirmation|null;showResult?:boolean;currentConversationId?:string;compact?:boolean}) {
  const { t } = useTranslation();
  const [openFailed,setOpenFailed] = useState<string|null>(null);
  const [pending,setPending] = useState<string|null>(null);
  const [controlError,setControlError] = useState<{scope:string;code:string}|null>(null);
  const [now, setNow] = useState(Date.now());

  useEffect(() => {
    setNow(Date.now());
    if (!confirmation?.expires_at_ms) return;
    const timer = setInterval(() => {
      setNow(Date.now());
    }, 250);
    return () => clearInterval(timer);
  }, [confirmation?.approval_id, confirmation?.expires_at_ms]);

  const isExpired = Boolean(confirmation && (!Number.isFinite(confirmation.expires_at_ms) || now >= confirmation.expires_at_ms));
  const remainingSec = confirmation?.expires_at_ms
    ? Math.max(0, Math.ceil((confirmation.expires_at_ms - now) / 1000))
    : 0;

  const scope=`${instanceId ?? ''}:${task?.command_id ?? ''}:${confirmation?.approval_id ?? ''}`;
  const currentScope=useRef(scope);currentScope.current=scope;
  const control = async (cancel:boolean) => {
    if (!task || !instanceId) return;
    const requestScope=scope;
    setPending(requestScope);setControlError(null);
    try {if(cancel)await cancelVoiceTask(instanceId,task);else await getVoiceTask(task.command_id);}
    catch(cause) {if(currentScope.current===requestScope)setControlError({scope:requestScope,code:cause instanceof Error?cause.message:'task_control_unknown'});}
    finally {setPending((value) => value===requestScope?null:value);}
  };
  const canCancel=task && task.status === 'accepted' && task.conversation_id && task.message_id;
  const decide=async(approved:boolean)=>{
    if(!task || !instanceId || !confirmation || Date.now()>=confirmation.expires_at_ms)return;
    const requestScope=scope;setPending(requestScope);setControlError(null);
    try {await confirmVoiceTask(instanceId,task,confirmation,approved);}
    catch(cause){if(currentScope.current===requestScope)setControlError({scope:requestScope,code:cause instanceof Error?cause.message:'task_control_unknown'});}
    finally{setPending((value)=>value===requestScope?null:value);}
  };
  if (!task && !errorCode) return null;
  return <article className='flex flex-col gap-2'>
    {!compact && <h2 className='text-base font-semibold m-0'>{t('voice.task.title')}</h2>}
    {task && <>
      <p role='status' className='m-0 text-t-secondary'>{t(`voice.task.status.${task.status}`)}</p>
      {showResult && task.result_text && <p className='m-0 whitespace-pre-wrap break-words'>{task.result_text}</p>}
      {confirmation && task.status==='accepted' && <section className='flex flex-col gap-2'>
        <h3 className='m-0 font-semibold'>{t('voice.task.confirmationTitle')}</h3>
        <p className='m-0 text-t-secondary'>{t('voice.task.confirmationHint')}</p>
        <div className='font-semibold'>{confirmation.tool_name}</div>
        <details><summary className='cursor-pointer text-t-secondary'>{t('voice.task.toolArguments')}</summary><pre className='m-0 text-12px whitespace-pre-wrap break-words'>{JSON.stringify(confirmation.arguments,null,2)}</pre></details>
        <div className='flex flex-wrap gap-3'>
          <Button type='primary' disabled={!instanceId || pending===scope || isExpired} onClick={()=>{void decide(true);}}>
            {t('voice.task.allowOnce')}
            {remainingSec > 0 ? ` (${remainingSec}s)` : ''}
          </Button>
          <Button disabled={!instanceId || pending===scope || isExpired} onClick={()=>{void decide(false);}}>{t('voice.task.deny')}</Button>
        </div>
      </section>}
      <div className='flex flex-wrap gap-3'>
        {task.conversation_id && task.conversation_id !== currentConversationId && <Button onClick={() => {void openVoiceTask(task.conversation_id!).catch(() => {if(currentScope.current===scope)setOpenFailed(scope);});}}>{t('voice.task.open')}</Button>}
        {instanceId && (!compact || task.status === 'unknown') && <Button disabled={pending===scope} onClick={() => {void control(false);}}>{t('voice.task.refresh')}</Button>}
        {instanceId && canCancel && <Button status='danger' disabled={pending===scope} onClick={() => {void control(true);}}>{t('voice.task.cancel')}</Button>}
      </div>
    </>}
    {(errorCode || task?.error_code) && <p role='alert' className='m-0 text-12px text-t-secondary'>{t(errorCode==='task_announcement_failed'?'voice.task.audioUnavailable':errorCode==='task_approval_detail_required'?'voice.task.confirmationDetailRequired':'voice.task.failedHint')}</p>}
    {openFailed===scope && <p role='alert' className='m-0 text-12px text-t-secondary'>{t('voice.task.openFailed')}</p>}
    {controlError?.scope===scope && <p role='alert' className='m-0 text-12px text-t-secondary'>{t(controlError.code==='task_scope_changed'||controlError.code==='task_approval_changed'?'voice.task.scopeChanged':'voice.task.controlUnknown')}</p>}
  </article>;
}
