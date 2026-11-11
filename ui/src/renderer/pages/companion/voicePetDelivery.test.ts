import {expect,test} from 'bun:test';
import {deliverVoicePetText} from './voicePetDelivery';

const conversationId='019a1234-1234-7123-8123-123456789abc';
const fails = async (promise:Promise<unknown>,message:string) => expect(await promise.then(()=>null,error=>(error as Error).message)).toBe(message);
const storage=()=>{const data=new Map<string,string>();return {getItem:(k:string)=>data.get(k)??null,setItem:(k:string,v:string)=>{data.set(k,v);},removeItem:(k:string)=>{data.delete(k);}};};
test('lost acceptance is resolved without executing twice or replacing the original payload',async()=>{
  const store=storage();const sends:string[][]=[];
  const ports={getTask:async()=>({conversation_id:conversationId}),send:async(...args:string[])=>{sends.push(args);throw new Error('response_lost');}};
  await fails(deliverVoicePetText(store,'delivery',conversationId,'original',ports),'response_lost');
  const result=await deliverVoicePetText(store,'delivery',conversationId,'new draft',ports);
  expect(sends.length).toBe(1);expect(result?.input).toBe('original');expect(store.getItem('delivery')).toBeNull();
});
test('unknown query keeps the pending command and cannot dispatch a replacement',async()=>{
  const store=storage();let count=0;
  const ports={getTask:async()=>{throw new Error('offline');},send:async()=>{count++;throw new Error('offline');}};
  await fails(deliverVoicePetText(store,'delivery',conversationId,'first',ports),'offline');
  const saved=store.getItem('delivery');
  await fails(deliverVoicePetText(store,'delivery',conversationId,'second',ports),'offline');
  expect(count).toBe(1);expect(store.getItem('delivery')).toBe(saved);
});
test('definite absence retries the same command; a later deliberate identical input gets a new identity',async()=>{
  const store=storage();const ids:string[]=[];
  const ports={getTask:async()=>{throw new Error('task_not_found');},send:async(_c:string,id:string)=>{ids.push(id);if(ids.length===1)throw new Error('lost');}};
  await fails(deliverVoicePetText(store,'delivery',conversationId,'same',ports),'lost');
  await deliverVoicePetText(store,'delivery',conversationId,'same',ports);
  await deliverVoicePetText(store,'delivery',conversationId,'same',ports);
  expect(ids[0]).toBe(ids[1]);expect(ids[2]).not.toBe(ids[0]);
});
