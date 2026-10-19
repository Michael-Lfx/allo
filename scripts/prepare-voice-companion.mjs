#!/usr/bin/env bun
// Tauri beforeBuildCommand: compile the production daemon before collecting resources.
import { spawnSync } from 'node:child_process';
import { copyFile, mkdir, chmod } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { resolveRgTarget } from './ensure-bundled-rg.mjs';

const root=join(dirname(fileURLToPath(import.meta.url)),'..');

export function voiceBuildPlan(env=process.env) {
  const target=resolveRgTarget(env);
  return {
    triples:target.arch==='universal'?['aarch64-apple-darwin','x86_64-apple-darwin']:[target.triple],
    profile:env.TAURI_ENV_DEBUG==='true'?'debug':'release',
    binary:target.platform==='win32'?'ai-mate.exe':'ai-mate',
  };
}

export function builtVoiceExecutable(output) {
  let executable;
  for (const line of output.split('\n')) {
    let item;try {item=JSON.parse(line);} catch {continue;}
    if (item.reason!=='compiler-artifact' || item.target?.name!=='ai-mate' || !item.target.kind.includes('bin')) continue;
    if (item.features?.includes('voice-skeleton')) throw new Error('Refusing to package a voice-skeleton executable');
    if (item.executable) executable=item.executable;
  }
  if (!executable) throw new Error('Cargo did not produce the ai-mate executable');
  return executable;
}

export async function stageVoiceCompanion(executable,outDir,binary,licenses) {
  await mkdir(join(outDir,'voice-licenses'),{recursive:true});
  await copyFile(executable,join(outDir,binary));
  if (!binary.endsWith('.exe')) await chmod(join(outDir,binary),0o755);
  for (const [name,path] of Object.entries(licenses)) await copyFile(path,join(outDir,'voice-licenses',name));
}

async function main() {
  const plan=voiceBuildPlan();
  const executables=[];
  for (const triple of plan.triples) {
    console.log(`[voice-package] building ${triple} ${plan.profile}`);
    const result=spawnSync('cargo',['build','-p','ai-mate','--bin','ai-mate','--no-default-features',
      '--target',triple,'--message-format=json-render-diagnostics',...(plan.profile==='release'?['--release']:[])],
      {cwd:root,encoding:'utf8',stdio:['ignore','pipe','inherit'],windowsHide:true,maxBuffer:64*1024*1024});
    if (result.error) throw result.error;
    if (result.status!==0) {
      process.stderr.write(result.stdout||'');
      throw new Error(`ai-mate build failed for ${triple}: ${result.status}`);
    }
    executables.push(builtVoiceExecutable(result.stdout));
  }
  const outDir=join(root,'apps','desktop','resources','bin');
  let executable=executables[0];
  if (executables.length>1) {
    executable=join(dirname(executables[0]),'ai-mate-universal');
    const result=spawnSync('lipo',['-create',...executables,'-output',executable],{stdio:'inherit',windowsHide:true});
    if (result.error) throw result.error;
    if (result.status!==0) throw new Error('lipo could not assemble the universal voice daemon');
  }
  await stageVoiceCompanion(executable,outDir,plan.binary,{
    'Flowy-LICENSE':join(root,'LICENSE'),
    'sherpa-onnx-LICENSE':join(root,'crates','vendor','sherpa-onnx','LICENSE'),
  });
  console.log(`[voice-package] staged ${join(outDir,plan.binary)}`);
}

if (import.meta.main) main().catch(error=>{console.error(error);process.exit(1);});
