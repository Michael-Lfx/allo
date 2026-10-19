import { expect, test } from 'bun:test';
import { voiceModelOptions } from './voiceModelOptions';

test('voice chooses existing chat catalog identities and excludes unsupported protocols',()=>{
  const options=voiceModelOptions([
    {provider:{id:'cloud',platform:'openai'},models:['AIPC-qwen3.8-flash']},
    {provider:{id:'local',platform:'openai',model_protocols:{unsupported:'openai-responses'}},models:['local-chat','unsupported']},
    {provider:{id:'other',platform:'anthropic'},models:['other-chat']},
  ]);
  expect(options).toEqual([{provider_id:'cloud',model:'AIPC-qwen3.8-flash'},{provider_id:'local',model:'local-chat'}]);
});
