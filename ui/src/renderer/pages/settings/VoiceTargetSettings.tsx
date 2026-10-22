import React,{useEffect,useState} from 'react';
import {Select} from '@arco-design/web-react';
import {useTranslation} from 'react-i18next';
import {ipcBridge} from '@common/index';
import {getVoiceTargetSettings,saveVoiceTargetSettings,type VoiceTargetSettings} from '@common/adapter/voiceCompanion';
import {SettingsSection,SettingsRow,SettingsList} from '@renderer/components/settings/SettingsPagePrimitives';

export default function VoiceTargetSettingsSection(){
  const {t}=useTranslation();
  const [settings,setSettings]=useState<VoiceTargetSettings|null>(null);
  const [pets,setPets]=useState<Array<{companion_id:string;name:string}>>([]);
  const [failed,setFailed]=useState(false);
  const [busy,setBusy]=useState(false);
  useEffect(()=>{let alive=true;void Promise.all([getVoiceTargetSettings(),ipcBridge.companion.listCompanions.invoke()]).then(([value,list])=>{
    if(alive){setSettings(value);setPets(list);}
  }).catch(()=>{if(alive)setFailed(true);});return()=>{alive=false;};},[]);
  const value=settings?.requested.mode==='companion'?settings.requested.companion_id??'home':'home';
  return <SettingsSection title={t('voice.target.title')} description={t('voice.target.hint')}><SettingsList>
    <SettingsRow label={t('voice.target.default')} description={failed?t('voice.target.failed'):settings?.reason?t('voice.target.fallback'):t('voice.target.frozen')}
      controlLayout='field' control={<Select aria-label={t('voice.target.default')} value={value} disabled={!settings||busy} onChange={(id:string)=>{
        setBusy(true);setFailed(false);void saveVoiceTargetSettings(id==='home'?{mode:'voice_home'}:{mode:'companion',companion_id:id}).then(setSettings).catch(()=>setFailed(true)).finally(()=>setBusy(false));
      }}><Select.Option value='home'>{t('voice.target.home')}</Select.Option>
        {value!=='home'&&!pets.some(p=>p.companion_id===value)&&<Select.Option value={value} disabled>{t('voice.target.missing')}</Select.Option>}
        {pets.map(p=><Select.Option key={p.companion_id} value={p.companion_id}>{p.name}</Select.Option>)}
      </Select>}/>
  </SettingsList></SettingsSection>;
}
