import { expect, test } from 'bun:test';
import { mkdtemp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { voiceBuildPlan, stageVoiceCompanion, builtVoiceExecutable } from './prepare-voice-companion.mjs';

test('voice packaging uses the same target and debug intent as the desktop hook', () => {
  const debug=voiceBuildPlan({TAURI_ENV_TARGET_TRIPLE:'aarch64-pc-windows-msvc',TAURI_ENV_DEBUG:'true'});
  expect(debug.triples).toEqual(['aarch64-pc-windows-msvc']);
  expect(debug.profile).toBe('debug');
  expect(debug.binary).toBe('ai-mate.exe');
  const release=voiceBuildPlan({TAURI_ENV_TARGET_TRIPLE:'x86_64-unknown-linux-gnu',TAURI_ENV_DEBUG:'false'});
  expect(release.profile).toBe('release');
  expect(release.binary).toBe('ai-mate');
  expect(voiceBuildPlan({TAURI_ENV_TARGET_TRIPLE:'universal-apple-darwin'}).triples)
    .toEqual(['aarch64-apple-darwin','x86_64-apple-darwin']);
});

test('packaging takes the actual Cargo executable and refuses skeleton or missing artifacts', () => {
  const artifact={reason:'compiler-artifact',target:{name:'ai-mate',kind:['bin']},features:[],executable:'C:/isolated/ai-mate.exe'};
  expect(builtVoiceExecutable(JSON.stringify(artifact))).toBe(artifact.executable);
  expect(()=>builtVoiceExecutable(JSON.stringify({...artifact,features:['voice-skeleton']}))).toThrow('skeleton');
  expect(()=>builtVoiceExecutable(JSON.stringify({...artifact,executable:null}))).toThrow('executable');
});

test('voice resources include the actual executable and existing licenses; failure cannot reuse an old binary', async () => {
  const root=await mkdtemp(join(tmpdir(),'flowy-voice-package-'));
  try {
    const source=join(root,'build','ai-mate.exe');
    const out=join(root,'bin');
    await mkdir(join(root,'build'));await writeFile(source,'test-only artifact');
    const license=join(root,'LICENSE');await writeFile(license,'test-only license fixture');
    await stageVoiceCompanion(source,out,'ai-mate.exe',{'LICENSE':license});
    expect(await readFile(join(out,'ai-mate.exe'),'utf8')).toBe('test-only artifact');
    expect(await readFile(join(out,'voice-licenses','LICENSE'),'utf8')).toBe('test-only license fixture');
    await expect(stageVoiceCompanion(join(root,'missing'),out,'ai-mate.exe',{'LICENSE':license})).rejects.toThrow();
  } finally {await rm(root,{recursive:true,force:true});}
});
