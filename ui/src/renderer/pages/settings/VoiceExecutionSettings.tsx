import React, { useEffect, useRef, useState } from 'react';
import { Button, Input, Select } from '@arco-design/web-react';
import { useTranslation } from 'react-i18next';
import { ipcBridge } from '@common/index';
import type { Preset } from '@common/types/agent/presetTypes';
import { SettingsList, SettingsRow, SettingsSection } from '@renderer/components/settings/SettingsPagePrimitives';
import { getVoiceExecutionSettings, saveVoiceExecutionSettings, type VoiceExecutionConfig, type VoiceExecutionSettings, type VoiceModelRef } from '@common/adapter/voiceCompanion';

export default function VoiceExecutionSettingsSection({ connected, models }: {connected:boolean;models:Array<VoiceModelRef & {label:string}>}) {
  const { t, i18n } = useTranslation();
  const [settings,setSettings] = useState<VoiceExecutionSettings|null>(null);
  const [draft,setDraft] = useState<VoiceExecutionConfig|null>(null);
  const [presets,setPresets] = useState<Preset[]>([]);
  const [error,setError] = useState(false);
  const [catalogError,setCatalogError] = useState(false);
  const [busy,setBusy] = useState(false);
  const [refresh,setRefresh] = useState(0);
  const revision = useRef(0);
  useEffect(() => {
    let alive = true;
    const load = ++revision.current;
    void getVoiceExecutionSettings().then((value) => {
      if (alive && load===revision.current) {
        setSettings(value); setDraft((previous) => previous ?? value.requested ?? {workspace:'',model:null,preset_id:null}); setError(false);
      }
    }).catch(() => {if(alive && load===revision.current)setError(true);});
    void ipcBridge.presets.list.invoke().then((list) => {
      if (alive) {setPresets(list.filter((preset) => preset.enabled && preset.targets.includes('conversation')));setCatalogError(false);}
    }).catch(() => {if(alive)setCatalogError(true);});
    return () => {alive=false;};
  },[connected,refresh]);
  const save = async () => {
    if (!draft) return;
    ++revision.current;setBusy(true);
    try {setSettings(await saveVoiceExecutionSettings(draft));setError(false);}
    catch {setError(true);}
    finally {setBusy(false);}
  };
  const modelValue = draft?.model ? JSON.stringify([draft.model.provider_id,draft.model.model]) : 'preset';
  const staleModel = draft?.model && !models.some((model) => JSON.stringify([model.provider_id,model.model])===modelValue);
  const stalePreset = draft?.preset_id && !presets.some((preset) => preset.preset_id===draft.preset_id);
  const dirty = Boolean(settings && draft && JSON.stringify(settings.requested)!==JSON.stringify(draft));
  const ready = settings?.available && !dirty;
  return <SettingsSection title={t('voice.execution.title')} description={t('voice.execution.description')}>
    <SettingsList>
      <SettingsRow label={t('voice.execution.workspace')} description={t('voice.execution.workspaceHint')} controlLayout='compound' control={<div className='flex items-center gap-3'>
        <Input className='flex-1 min-w-0' aria-label={t('voice.execution.workspace')} value={draft?.workspace ?? ''} disabled={!draft || busy} onChange={(workspace) => setDraft((value) => value && {...value,workspace})}/>
        <Button disabled={!draft || busy} onClick={() => {void ipcBridge.dialog.showOpen.invoke({properties:['openDirectory']}).then((paths) => {if(paths?.[0])setDraft((value) => value && {...value,workspace:paths[0]});}).catch(() => setError(true));}}>{t('voice.execution.browse')}</Button>
      </div>}/>
      <SettingsRow label={t('voice.execution.model')} description={t('voice.execution.modelHint')} controlLayout='field' control={<Select aria-label={t('voice.execution.model')} value={modelValue} disabled={!draft || busy} onChange={(value:string) => {
        const model = models.find((model) => JSON.stringify([model.provider_id,model.model])===value);
        setDraft((previous) => previous && {...previous,model:model?{provider_id:model.provider_id,model:model.model}:null});
      }}>
        <Select.Option value='preset'>{t('voice.execution.presetModel')}</Select.Option>
        {staleModel && <Select.Option value={modelValue} disabled>{t('voice.dialogue.stale',{model:draft?.model?.model})}</Select.Option>}
        {models.map((model) => <Select.Option key={JSON.stringify([model.provider_id,model.model])} value={JSON.stringify([model.provider_id,model.model])}>{model.label}</Select.Option>)}
      </Select>}/>
      <SettingsRow label={t('voice.execution.preset')} description={catalogError?t('voice.execution.catalogFailed'):t('voice.execution.presetHint')} controlLayout='field' control={<Select aria-label={t('voice.execution.preset')} value={draft?.preset_id ?? 'none'} disabled={!draft || busy} onChange={(value:string) => setDraft((previous) => previous && {...previous,preset_id:value==='none'?null:value})}>
        <Select.Option value='none'>{t('voice.execution.noPreset')}</Select.Option>
        {stalePreset && <Select.Option value={draft!.preset_id!} disabled>{t('voice.execution.stalePreset')}</Select.Option>}
        {presets.map((preset) => <Select.Option key={preset.preset_id} value={preset.preset_id}>{preset.name_i18n[i18n.language] || preset.name}</Select.Option>)}
      </Select>}/>
      <SettingsRow label={t('voice.execution.status')} description={<span role={error?'alert':'status'}>{error?t('voice.execution.failed'):!settings?t('voice.feedback.loading'):dirty?t('voice.feedback.unsaved'):ready?t('voice.execution.ready'):t('voice.execution.unavailable')}</span>} controlLayout='actions' control={<div className='flex flex-wrap gap-3'>
        <Button disabled={busy} onClick={() => setRefresh((value) => value+1)}>{t('voice.service.retrySync')}</Button>
        <Button type='primary' loading={busy} disabled={!draft || !draft.workspace.trim() || (!draft.model && !draft.preset_id)} onClick={() => {void save();}}>{t('voice.execution.save')}</Button>
      </div>}/>
    </SettingsList>
  </SettingsSection>;
}
