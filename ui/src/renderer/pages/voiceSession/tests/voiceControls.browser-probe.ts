/** Isolated browser test entry; never imported by production routes. */
import React, {act} from 'react';
import {createRoot} from 'react-dom/client';
import {createInstance} from 'i18next';
import {I18nextProvider} from 'react-i18next';
import VoiceTaskQueue from '../VoiceTaskQueue';
import VoiceTaskStatus from '../VoiceTaskStatus';
import ComposerSubmitCluster from '@renderer/components/chat/ComposerSubmitCluster';
import {CloudAuthContext,type CloudAuthContextValue} from '@renderer/hooks/context/CloudAuthContext';
import * as voice from '@common/adapter/voiceCompanion';
import zh from '@renderer/services/i18n/locales/zh-CN/voice.json';
import en from '@renderer/services/i18n/locales/en-US/voice.json';

(globalThis as any).IS_REACT_ACT_ENVIRONMENT=true;
const result=document.createElement('pre');result.id='voice-control-result';document.body.append(result);
const cases:string[]=[];
const check=(condition:unknown,message:string)=>{if(!condition)throw new Error(message);};
const deferred=()=>{let resolve!:(value:Response)=>void;return {promise:new Promise<Response>(done=>{resolve=done;}),resolve:(value:Response)=>resolve(value)};};
const e=React.createElement;
const container=document.createElement('div');document.body.append(container);const root=createRoot(container);
const task=(id:string)=>({command_id:id,session_id:'s',acoustic_turn_id:'t',conversation_id:id,message_id:'m-'+id,runtime_turn_id:null,status:'accepted' as const,result_ok:null,result_text:null,error_code:null});

async function run(){
 for(const [locale,dictionary] of [['zh-CN',zh],['en-US',en]] as const){
  const i18n=createInstance();await i18n.init({lng:locale,resources:{[locale]:{translation:{voice:dictionary}}},initImmediate:false});
  const render=async(child:React.ReactNode)=>{await act(async()=>{root.render(e(I18nextProvider,{i18n},e(CloudAuthContext.Provider,{value:{ready:false,status:'unauthenticated'} as CloudAuthContextValue},child)));});};
  for(const kind of ['queue','approval'] as const){
   const calls:Array<{url:string;body:any;response:ReturnType<typeof deferred>}>=[];
   window.fetch=(async(input:RequestInfo|URL,init?:RequestInit)=>{
    const response=deferred();calls.push({url:String(input),body:JSON.parse(String(init?.body??'{}')),response});return response.promise;
   }) as typeof fetch;
   const renderTask=async(id:string)=>kind==='queue'
    ? render(e(VoiceTaskQueue,{instanceId:'host',conversationId:id,items:[{task:task(id),display_text:id,position:1,workspace:'D:/'+id}]}))
    : render(e(VoiceTaskStatus,{instanceId:'host',currentConversationId:id,task:task(id),errorCode:null,confirmation:{approval_id:'approval-'+id,msg_id:'permission-'+id,call_id:'tool-'+id,tool_name:'read',arguments:{},expires_at_ms:Date.now()+60_000}}));
   const button=()=>Array.from(container.querySelectorAll('button')).find(next=>next.textContent?.includes(kind==='queue'?dictionary.task.cancel:dictionary.task.allowOnce))!;
   await render(null);await renderTask('A');
   await act(async()=>{button().click();});
   check(calls.length===1&&calls[0].body.conversation_id==='A',kind+' uses A identity');check(button().disabled,kind+' A pending');
   await renderTask('B');check(!button().disabled,kind+' B starts unlocked');
   await act(async()=>{button().click();});check(calls.length===2&&calls[1].body.conversation_id==='B',kind+' uses B identity');
   await act(async()=>{calls[0].response.resolve(Response.json({code:'task_scope_changed'},{status:409}));});
   check(button().disabled,kind+' A settlement cannot unlock B');check(!container.querySelector('[role=alert]'),kind+' A error cannot leak to B');
   await act(async()=>{calls[1].response.resolve(Response.json({success:true}));});check(!button().disabled,kind+' B settles');
   cases.push(locale+': '+kind+' A click → B click → late A failure → B settlement');
  }
  let attempts=0;const requests:any[]=[];
  window.fetch=(async(input:RequestInfo|URL,init?:RequestInit)=>{
   if(!String(input).endsWith('/session/start'))return Response.json({available:false});
   const request=JSON.parse(String(init?.body));requests.push(request);attempts++;
   return Response.json({request_id:request.request_id,status:attempts===1?'starting':'ready',conversation_id:'resolved',session_id:attempts===1?null:'session'});
  }) as typeof fetch;
  await voice.startVoiceCallIntent({kind:'current_selection'},'auto_edit');
  let retry:Promise<unknown>|undefined;
  await render(e(ComposerSubmitCluster,{hasDraft:false,isVoiceConversation:true,isVoiceStartUncertain:voice.getPendingVoiceCallConversationId()==='resolved',onSend:()=>{},onSpeechTranscript:()=>{},onStartVoiceCall:()=>{retry=voice.startVoiceCallIntent({kind:'voice_conversation',conversation_id:'resolved'});}}));
  const retryButton=container.querySelector('[data-testid=composer-phone-btn]') as HTMLButtonElement;
  check(retryButton?.getAttribute('aria-label')===dictionary.composer.retryStart,'starting is visibly recoverable');
  await act(async()=>{retryButton.click();await retry;});check(requests.length===2&&JSON.stringify(requests[0])===JSON.stringify(requests[1]),'route retry preserves exact request');
  cases.push(locale+': starting receipt → resolved composer retry click → original request');
 }
 await act(async()=>{root.unmount();});
 result.textContent=JSON.stringify({ok:true,cases});result.dataset.ready='true';
}
void run().catch(error=>{result.textContent=JSON.stringify({ok:false,cases,error:String(error),stack:error?.stack});result.dataset.ready='true';});
