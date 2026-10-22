import type { VoiceModelRef } from '@common/adapter/voiceCompanion';
export function voiceModelOptions(groups: readonly {provider:{id:string;platform:string;model_protocols?:Record<string,string>};models:string[]}[]):VoiceModelRef[] {
  return groups.filter(({provider})=>provider.platform==='openai').flatMap(({provider,models})=>models.filter((model)=>!provider.model_protocols?.[model] || provider.model_protocols[model]==='openai').map((model)=>({provider_id:provider.id,model})));
}
