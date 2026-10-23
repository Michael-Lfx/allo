import {expect,test} from 'bun:test';
import {stopDisplayedVoiceTask} from './stopVoiceTask';
import type {VoiceSnapshot,VoiceTaskSnapshot} from '@common/adapter/voiceCompanion';
const task=(id:string,status:VoiceTaskSnapshot['status']='accepted'):VoiceTaskSnapshot=>({command_id:id,session_id:'s',acoustic_turn_id:'t',conversation_id:'p',message_id:`msg-${id}`,runtime_turn_id:`rt-${id}`,status,result_ok:null,result_text:null,error_code:null});
test('primary voice stop freezes A and never uses the ordinary Conversation stop or cancels queued B',async()=>{
  const original=globalThis.fetch;const calls:Array<{url:string;body:unknown}>=[];
  const a=task('a');const snapshot={instance_id:'host',task:a,queue:[{task:task('b','queued')}]} as unknown as VoiceSnapshot;
  let release:()=>void=()=>{};const response=new Promise<void>(resolve=>{release=resolve;});
  globalThis.fetch=(async(input:RequestInfo|URL,init?:RequestInit)=>{calls.push({url:String(input),body:JSON.parse(String(init?.body))});await response;return Response.json({...a,status:'cancelling'});}) as typeof fetch;
  try{
    const stopped=stopDisplayedVoiceTask('p',snapshot);
    snapshot.task=task('b');release();
    expect((await stopped).status).toBe('cancelling');expect(calls.length).toBe(1);
    expect(calls[0].url.endsWith('/tasks/a/cancel')).toBe(true);
    expect(calls[0].body).toMatchObject({instance_id:'host',command_id:'a',conversation_id:'p',message_id:'msg-a'});
    expect(snapshot.queue?.[0].task.command_id).toBe('b');
  }finally{globalThis.fetch=original;}
});
test('unknown, delivering and cancelling commands are queried; foreign or missing scope never stops a Conversation',async()=>{
  const original=globalThis.fetch;const methods:string[]=[];
  globalThis.fetch=(async(_input:RequestInfo|URL,init?:RequestInit)=>{methods.push(init?.method??'GET');return Response.json(task('a','cancelling'));}) as typeof fetch;
  try{
    for(const status of ['unknown','delivering','cancelling'] as const)await stopDisplayedVoiceTask('p',{instance_id:'host',task:task('a',status)} as VoiceSnapshot);
    expect(methods).toEqual(['GET','GET','GET']);
    expect(await stopDisplayedVoiceTask('q',{instance_id:'host',task:task('a')} as VoiceSnapshot).then(()=>null,error=>(error as Error).message)).toBe('task_scope_changed');
    expect(methods.length).toBe(3);
    expect((await stopDisplayedVoiceTask('p',{instance_id:'host',task:task('a','completed')} as VoiceSnapshot)).status).toBe('completed');
    expect(methods.length).toBe(3);
  }finally{globalThis.fetch=original;}
});
