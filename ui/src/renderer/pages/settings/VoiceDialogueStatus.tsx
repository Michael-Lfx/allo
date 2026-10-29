import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { getVoiceDialogueSettings, type VoiceDialogueSettings } from '@common/adapter/voiceCompanion';
import { SettingsList, SettingsRow, SettingsSection } from '@renderer/components/settings/SettingsPagePrimitives';

export function VoiceDialogueStatus({ settings, failed = false }: { settings: VoiceDialogueSettings | null; failed?: boolean }) {
  const { t } = useTranslation();
  return <SettingsSection title={t('voice.dialogue.title')} description={t('voice.dialogue.description')}><SettingsList>
    <SettingsRow label={t('voice.dialogue.model')} controlLayout='field' control={
      <span className='break-words text-13px text-t-secondary' role='status'>
        {failed ? t('voice.dialogue.failed') : !settings ? t('voice.feedback.loading') : settings.available && settings.effective_model
          ? t('voice.dialogue.selected', { provider: settings.effective_model.provider_id, model: settings.effective_model.model })
          : t('voice.dialogue.unavailable')}
      </span>
    } />
  </SettingsList></SettingsSection>;
}

export default function VoiceDialogueStatusSection() {
  const [settings, setSettings] = useState<VoiceDialogueSettings | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let alive = true;
    void getVoiceDialogueSettings().then(value => { if (alive) setSettings(value); }).catch(() => { if (alive) setFailed(true); });
    return () => { alive = false; };
  }, []);
  return <VoiceDialogueStatus settings={settings} failed={failed} />;
}
