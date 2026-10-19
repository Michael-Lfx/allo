import {expect,test} from 'bun:test';
import React from 'react';
import {renderToStaticMarkup} from 'react-dom/server';
import {createInstance} from 'i18next';
import {I18nextProvider} from 'react-i18next';
import VoiceTaskDiagnostics from './VoiceTaskDiagnostics';
import VoiceErrorStatus from './VoiceErrorStatus';
import VoiceTargetSettings from '../settings/VoiceTargetSettings';
import zh from '@renderer/services/i18n/locales/zh-CN/voice.json';
import en from '@renderer/services/i18n/locales/en-US/voice.json';
import type {VoiceSnapshot} from '@common/adapter/voiceCompanion';
for(const [locale,dict] of [['zh-CN',zh],['en-US',en]] as const){
  test(locale+': execution, health and actual playback stay distinct',async()=>{
    const i18n=createInstance();await i18n.init({lng:locale,resources:{[locale]:{translation:{voice:dict}}},initImmediate:false});
    for(const status of ['pending','dispatched','started','failed','played','unknown','suppressed'] as const){
      const snapshot={instance_id:'host',enabled:true,connection:'connected',session_id:'s',turn_id:'t',binding:{session_id:'s',binding_id:'b',conversation_id:'c',workspace:'D:/owned'},task:{command_id:'command',status:'completed'},task_progress:{command_id:'command',stage:'tool',health:'no_recent_progress'},task_notification:{command_id:'command',status}} as unknown as VoiceSnapshot;
      const html=renderToStaticMarkup(<I18nextProvider i18n={i18n}><VoiceTaskDiagnostics snapshot={snapshot}/></I18nextProvider>);
      expect(html).toContain(dict.notification[status]);expect(html).toContain(dict.progress.no_recent_progress);expect(html).toContain('D:/owned');expect(html).toContain(dict.notification.replay);expect(html).not.toContain('voice.');
      if(status==='failed')expect(html).not.toContain(dict.notification.played);
      expect(html.includes('disabled=')).toBe(status==='dispatched'||status==='started');
    }
  });
  test(locale+': model, binding and persistence failures have separate messages',async()=>{
    const i18n=createInstance();await i18n.init({lng:locale,resources:{[locale]:{translation:{voice:dict}}},initImmediate:false});
    for(const [code,key] of [['model_timeout','modelTimeout'],['binding_invalid','bindingInvalid'],['binding_busy','bindingBusy'],['voice_record_conflict','recordConflict'],['voice_record_write_failed','recordFailed'],['voice_projection_write_failed','projectionFailed'],['voice_generation_unknown','generationUnknown']] as const){
      const html=renderToStaticMarkup(<I18nextProvider i18n={i18n}><VoiceErrorStatus error={{stage:'dialogue',code}}/></I18nextProvider>);
      expect(html).toContain(dict.audio[key]);expect(html).not.toContain(dict.audio.modelInvalid);expect(html).not.toContain('voice.');
    }
    const empty=renderToStaticMarkup(<I18nextProvider i18n={i18n}><VoiceErrorStatus error={{stage:'task',code:'task_not_found'}}/></I18nextProvider>);
    expect(empty).toContain(dict.task.noActive);
  });
  test(locale+': standalone target does not require creating a pet',async()=>{
    const i18n=createInstance();await i18n.init({lng:locale,resources:{[locale]:{translation:{voice:dict}}},initImmediate:false});
    const html=renderToStaticMarkup(<I18nextProvider i18n={i18n}><VoiceTargetSettings/></I18nextProvider>);
    expect(html).toContain(dict.target.title);expect(html).toContain(dict.target.hint);expect(html).not.toContain('voice.');
  });
}
