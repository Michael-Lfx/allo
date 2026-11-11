import {expect,test} from 'bun:test';
import React from 'react';
import {renderToStaticMarkup} from 'react-dom/server';
import {createInstance} from 'i18next';
import {I18nextProvider} from 'react-i18next';
import PetStatusToast from './PetStatusToast';
import zh from '@renderer/services/i18n/locales/zh-CN/voice.json';
import en from '@renderer/services/i18n/locales/en-US/voice.json';
import type {PetMessage} from './types';

for(const [locale,dictionary] of [['zh-CN',zh],['en-US',en]] as const){
  test(`${locale}: pet shows one primary state and keeps urgent confirmation ahead of other work`,async()=>{
    const i18n=createInstance();await i18n.init({lng:locale,resources:{[locale]:{translation:{voice:dictionary,nomi:{petMessage:{sources:{conversation:'Conversation'},dismiss:'Dismiss'}}}}},initImmediate:false});
    const base:PetMessage={id:'voice:a',source:'conversation',phase:'running',titleKey:'voice.session.phase.speaking',href:'/conversation/voice',updatedAt:1};
    const html=renderToStaticMarkup(<I18nextProvider i18n={i18n}><PetStatusToast messages={[base,{...base,id:'task:approval',phase:'waiting',titleKey:'voice.task.confirmationTitle'},{...base,id:'task:b'}]} onDismiss={()=>{}}/></I18nextProvider>);
    expect(html.match(/class="nomi-pet-chip /g)).toHaveLength(1);
    expect(html).toContain(dictionary.task.confirmationTitle);
    expect(html).toContain(dictionary.pet.more.replace('{{count}}','2'));
    expect(html).not.toContain(dictionary.session.phase.speaking);
    expect(html).not.toContain('voice.');
  });
}
