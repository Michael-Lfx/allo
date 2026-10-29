import React from 'react';
import {test,expect} from 'bun:test';
import {renderToStaticMarkup} from 'react-dom/server';
import {createInstance} from 'i18next';
import {I18nextProvider} from 'react-i18next';
import zh from '../../services/i18n/locales/zh-CN/voice.json';
import en from '../../services/i18n/locales/en-US/voice.json';
import VoiceTaskQueue from './VoiceTaskQueue';
import type {VoiceTaskSnapshot} from '@common/adapter/voiceCompanion';

for(const [locale,dictionary] of [['zh-CN',zh],['en-US',en]] as const){
  test(`${locale}: durable queue exposes each identity, position and cancellation`,async()=>{
    const i18n=createInstance();await i18n.init({lng:locale,resources:{[locale]:{translation:{voice:dictionary}}},initImmediate:false});
    const task:VoiceTaskSnapshot={command_id:'a',session_id:'s',acoustic_turn_id:'t',conversation_id:'c',message_id:'m-a',runtime_turn_id:null,status:'unknown',result_ok:null,result_text:null,error_code:null};
    const html=renderToStaticMarkup(<I18nextProvider i18n={i18n}><VoiceTaskQueue instanceId='host' items={[
      {task,display_text:'用户原文 A',workspace:'C:/workspace/P',position:0},
      {task:{...task,command_id:'b',message_id:'m-b',status:'queued'},display_text:'用户原文 B',workspace:'C:/workspace/P',position:1},
    ]}/></I18nextProvider>);
    expect(html).toContain('用户原文 A');expect(html).toContain('用户原文 B');expect(html).toContain('C:/workspace/P');
    expect(html).toContain(locale==='zh-CN'?'等待第 1 项':'Waiting: 1');
    expect(html).toContain(locale==='zh-CN'?'接单状态待核对':'Acceptance not yet verified');
    expect(html.match(/<button/g)?.length).toBe(2);
    expect(html).not.toContain('voice.');
  });
}
