import {describe,expect,test} from 'bun:test';
import {readFileSync} from 'node:fs';
import zh from '@/renderer/services/i18n/locales/zh-CN/nomi.json';
import en from '@/renderer/services/i18n/locales/en-US/nomi.json';
const overview=readFileSync(new URL('./index.tsx',import.meta.url),'utf8');
const chat=readFileSync(new URL('../../../companion/CompanionChatPanel.tsx',import.meta.url),'utf8');
describe('companion settings keep only missing-model onboarding',()=>{
 test('configured overview has no model/voice configuration section',()=>{
  expect(overview).not.toContain('ModelsSection');
  expect(overview).not.toContain('TaskModelSelect');
  expect(overview).not.toContain('voice.tts');
 });
 test('a new unconfigured companion can still select its chat model',()=>{
  expect(overview).toContain('modelMissing&&');
  expect(overview).toContain('<CompanionModelControl');
  expect(chat).toContain('CompanionModelControl');
 });
 test('private chat model authority remains in its original component',()=>{
  const control=readFileSync(new URL('../../../CompanionModelControl.tsx',import.meta.url),'utf8');
  expect(control).toContain('patchCompanion');
  expect(control).toContain('model');
 });
 test('missing-model guidance is translated in both languages',()=>{
  for(const dictionary of [zh,en])expect(dictionary.overview.modelMissing.length).toBeGreaterThan(0);
 });
});
