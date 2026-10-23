import React, {useRef,useState} from 'react';
import {useTranslation} from 'react-i18next';
import {getVoiceTask,openVoiceTask,sendVoiceKeyboardMessage} from '@common/adapter/voiceCompanion';
import {useComposerDraftStore} from '@renderer/stores/composerDraftStore';
import {deliverVoicePetText} from './voicePetDelivery';

/** This surface belongs only to its frozen voice target; the ordinary pet draft is untouched. */
export default function VoicePetInput({conversationId,workspace}:{conversationId:string;workspace:string|null}) {
  const {t}=useTranslation();
  const key=`pet-voice:${conversationId}`;
  const deliveryKey=`${key}:delivery`;
  const text=useComposerDraftStore(state=>state.drafts[key]?.text??'');
  const setText=(value:string)=>useComposerDraftStore.getState().setDraft(key,{text:value,files:[],dir:workspace??''});
  const [failed,setFailed]=useState(false);
  const [busy,setBusy]=useState(false);
  const inFlight=useRef(false);
  const send=async()=>{
    if(inFlight.current || !text.trim())return;
    inFlight.current=true;setBusy(true);setFailed(false);
    try {
      const delivery=await deliverVoicePetText(sessionStorage,deliveryKey,conversationId,text,{getTask:getVoiceTask,send:sendVoiceKeyboardMessage});
      if(!delivery)return;
      if(useComposerDraftStore.getState().drafts[key]?.text.trim()===delivery.input)useComposerDraftStore.getState().clearDraft(key);
    }catch{setFailed(true);}
    finally{inFlight.current=false;setBusy(false);}
  };
  const label=workspace?.split(/[\\/]/).filter(Boolean).at(-1)??t('voice.target.workspace');
  return <div className='nomi-companion-voice-input' data-companion-hit>
    <button type='button' className='nomi-companion-voice-input__target' onClick={()=>{void openVoiceTask(conversationId).catch(()=>setFailed(true));}}>{t('voice.presentation.inputTarget',{workspace:label})}</button>
    <div className='flex items-center gap-2'>
      <input aria-label={t('voice.session.sendText')} value={text} disabled={busy} onChange={event=>setText(event.target.value)}
        placeholder={t('voice.presentation.inputPlaceholder')}
        onKeyDown={event=>{if(event.key==='Enter'&&!event.nativeEvent.isComposing){event.preventDefault();void send();}}}/>
      <button type='button' disabled={busy||!text.trim()} onClick={()=>{void send();}}>{t('nomi.companion.send')}</button>
    </div>
    {failed&&<span role='alert'>{t('voice.presentation.sendFailed')}</span>}
  </div>;
}
