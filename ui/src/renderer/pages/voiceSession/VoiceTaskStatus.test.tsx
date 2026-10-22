import { expect, test } from 'bun:test';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { createInstance } from 'i18next';
import { I18nextProvider } from 'react-i18next';
import VoiceTaskStatus from './VoiceTaskStatus';
import zh from '@renderer/services/i18n/locales/zh-CN/voice.json';
import en from '@renderer/services/i18n/locales/en-US/voice.json';
import type { VoiceTaskSnapshot } from '@common/adapter/voiceCompanion';

for (const [locale, dictionary] of [['zh-CN',zh],['en-US',en]] as const) {
  test(`${locale}: the actual pending tool offers one-time approval and refusal`,async()=>{
    const i18n=createInstance();await i18n.init({lng:locale,resources:{[locale]:{translation:{voice:dictionary}}},initImmediate:false});
    const task:VoiceTaskSnapshot={command_id:'delivery',session_id:'voice-session',acoustic_turn_id:'voice-turn',conversation_id:'conversation',message_id:'message',runtime_turn_id:'runtime-turn',status:'accepted',result_ok:null,result_text:null,error_code:null};
    const approval={approval_id:'actual-approval',msg_id:'actual-message',call_id:'actual-call',tool_name:'Write',arguments:{file_path:'owned.txt'},expires_at_ms:Date.now()+8000};
    const html=renderToStaticMarkup(<I18nextProvider i18n={i18n}><VoiceTaskStatus task={task} instanceId='host' errorCode={null} confirmation={approval}/></I18nextProvider>);
    expect(html).toContain('owned.txt');expect(html).toContain('Write');
    expect(html).toContain(locale==='zh-CN'?'仅允许本次':'Allow once');
    expect(html).toContain(locale==='zh-CN'?'拒绝执行':'Deny');
    expect(html).not.toContain('voice.');expect(html).not.toContain('proceed_always');
  });
  test(`${locale}: real task results remain visible after the acoustic session ends`, async () => {
    const i18n = createInstance();
    await i18n.init({lng:locale,resources:{[locale]:{translation:{voice:dictionary}}},initImmediate:false});
    const task: VoiceTaskSnapshot = {command_id:'delivery',session_id:'voice-session',acoustic_turn_id:'voice-turn',
      conversation_id:'conversation',message_id:'message',runtime_turn_id:'runtime-turn',status:'completed',
      result_ok:true,result_text:'fresh workspace content\nsecond line',error_code:null};
    const render = (value: VoiceTaskSnapshot) => renderToStaticMarkup(<I18nextProvider i18n={i18n}><VoiceTaskStatus task={value} errorCode={null}/></I18nextProvider>);
    const html = render(task);
    expect(html).toContain('fresh workspace content');
    expect(html).toContain(locale==='zh-CN'?'查看完整任务':'Open full task');
    expect(html).not.toContain('voice.');
    const queued = render({...task,status:'queued',result_ok:null,result_text:null});
    expect(queued).toContain(locale==='zh-CN'?'任务已排队，等待当前任务结束':'Task queued; waiting for the active task to finish');
    expect(queued).not.toContain('voice.');
    const unknown = render({...task,status:'unknown',result_ok:null,result_text:null});
    expect(unknown).toContain(locale==='zh-CN'?'接单状态待核对':'Acceptance not yet verified');
    expect(unknown).not.toContain(locale==='zh-CN'?'任务已完成':'Task completed');
    const failed = render({...task,status:'failed',result_ok:false,error_code:'tool_denied',result_text:'read was denied'});
    expect(failed).toContain('read was denied');
    expect(failed).not.toContain(locale==='zh-CN'?'任务已完成':'Task completed');
  });
}
