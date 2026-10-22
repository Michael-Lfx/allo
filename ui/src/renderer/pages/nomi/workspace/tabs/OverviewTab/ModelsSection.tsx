/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React from 'react';
import { useTranslation } from 'react-i18next';
import type { ICompanionStatus } from '@/common/adapter/ipcBridge';
import { NomiSettingList, NomiSettingRow, NomiSettingSection } from '@/renderer/components/base/NomiSettingLayout';
import TaskModelSelect from '@/renderer/components/model/TaskModelSelect';
import CompanionModelControl from '@renderer/pages/nomi/CompanionModelControl';
import type { CompanionHandle } from '../../types';

interface ModelsSectionProps {
  companion: CompanionHandle;
  status: ICompanionStatus;
  companionName: string;
}

/**
 * 模型配置 — streamlined to essential companion preferences (Chat model & Speech voice).
 *
 * Technical parameters (VAD tuning, standalone vision model override, separate ASR
 * model slot, and cold disaster fallback) are removed from the overview tab to prevent
 * cognitive overload. Vision natively follows the conversation context, VAD uses
 * system optimal constants, and ASR is managed by the unified voice pipeline.
 * Repetitive "Flowy Cloud" provider selectors are hidden for a clean experience.
 */
const ModelsSection: React.FC<ModelsSectionProps> = ({ companion, status, companionName }) => {
  const { t } = useTranslation();
  const { profile, patchCompanion } = companion;

  if (!profile) return null;

  return (
    <NomiSettingSection
      title={t('nomi.overview.modelSection')}
      description={t('nomi.overview.modelSectionHint')}
    >
      <NomiSettingList>
        <NomiSettingRow
          title={t('nomi.overview.mainChatModel')}
          description={
            status.model_configured
              ? t('nomi.chat.modelConfigHint')
              : t('nomi.overview.modelMissing', { companionName })
          }
          style={status.model_configured ? undefined : { background: 'rgb(var(--warning-1))' }}
          controls={<CompanionModelControl companion={companion} showLabel={false} hideProvider />}
        />

        <NomiSettingRow
          title={t('nomi.overview.ttsSlot')}
          description={
            profile.voice.tts?.voice
              ? t('nomi.overview.ttsSlotHint')
              : t('nomi.overview.ttsFallback')
          }
          controls={
            <TaskModelSelect
              task='speech_synthesis'
              withVoice
              onlyVoice
              hideProvider
              value={profile.voice.tts}
              onChange={({ provider_id, model, voice }) =>
                void patchCompanion({ voice: { tts: { provider_id, model, voice: voice ?? null } } })
              }
            />
          }
        />
      </NomiSettingList>
    </NomiSettingSection>
  );
};

export default ModelsSection;
