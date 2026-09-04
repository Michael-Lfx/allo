/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useCallback, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, Input, Modal } from '@arco-design/web-react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import type { ICompanionProfile, ICustomPersona } from '@/common/adapter/ipcBridge';
import { NomiSettingList, NomiSettingRow, NomiSettingSection } from '@/renderer/components/base/NomiSettingLayout';
import NomiSelect from '@/renderer/components/base/NomiSelect';
import { useDebouncedText } from './useDebouncedText';
import type { CompanionHandle } from '../../types';

interface PersonaSectionProps {
  profile: ICompanionProfile;
  patchCompanion: CompanionHandle['patchCompanion'];
}

const BUILTIN_PERSONAS = ['lively', 'calm', 'sassy'] as const;
const MAX_CUSTOM_PERSONAS = 10;
const MAX_TITLE_CHARS = 20;
const MAX_BODY_CHARS = 2000;

/**
 * 伙伴设定 — how this companion talks: one selected persona (a built-in tone or one
 * of its own saved personas). Existing applied preset snapshots remain runtime
 * data, but the overview no longer exposes a second preset-selection surface.
 *
 * The selection and the custom-persona library are a single `persona` patch: the
 * backend stores `{ selected, customs }` together, and splitting the write would
 * let a deletion land without its re-selection.
 */
const PersonaSection: React.FC<PersonaSectionProps> = ({ profile, patchCompanion }) => {
  const { t } = useTranslation();
  const companionName = profile.name;
  const customs = profile.persona.customs ?? [];
  const selected = profile.persona.selected || 'lively';
  const selectedCustom = useMemo(() => customs.find((c) => c.id === selected) ?? null, [customs, selected]);

  const [createOpen, setCreateOpen] = useState(false);
  const [createTitle, setCreateTitle] = useState('');
  const [createBody, setCreateBody] = useState('');
  const [creating, setCreating] = useState(false);

  const [bodyDraft, onBodyChange] = useDebouncedText(selectedCustom?.body ?? '', (body) => {
    if (!selectedCustom) return;
    const trimmed = body.trim();
    if (!trimmed || trimmed === selectedCustom.body) return;
    const nextCustoms = customs.map((c) => (c.id === selectedCustom.id ? { ...c, body: trimmed } : c));
    void patchCompanion({ persona: { customs: nextCustoms } }).catch((e) => Message.error(String(e)));
  });

  const builtinLabel = (key: (typeof BUILTIN_PERSONAS)[number]) => {
    if (key === 'lively') return t('nomi.settings.personaLively', { defaultValue: '活泼' });
    if (key === 'calm') return t('nomi.settings.personaCalm', { defaultValue: '沉稳' });
    return t('nomi.settings.personaSassy', { defaultValue: '小毒舌' });
  };

  const submitCreate = useCallback(async () => {
    const title = createTitle.trim();
    const body = createBody.trim();
    if (!title) {
      Message.warning(t('nomi.settings.personaCustomTitleRequired'));
      return;
    }
    if (!body) {
      Message.warning(t('nomi.settings.personaCustomBodyRequired'));
      return;
    }
    if (customs.length >= MAX_CUSTOM_PERSONAS) {
      Message.warning(t('nomi.settings.personaCustomLimit', { max: MAX_CUSTOM_PERSONAS }));
      return;
    }
    const entry: ICustomPersona = {
      id: crypto.randomUUID(),
      title: title.slice(0, MAX_TITLE_CHARS),
      body: body.slice(0, MAX_BODY_CHARS),
    };
    setCreating(true);
    try {
      await patchCompanion({ persona: { selected: entry.id, customs: [...customs, entry] } });
      setCreateOpen(false);
    } catch (e) {
      Message.error(String(e));
    } finally {
      setCreating(false);
    }
  }, [createTitle, createBody, customs, patchCompanion, t]);

  const deleteSelectedCustom = useCallback(() => {
    if (!selectedCustom) return;
    Modal.confirm({
      title: t('nomi.settings.personaCustomDeleteTitle'),
      content: t('nomi.settings.personaCustomDeleteBody', { title: selectedCustom.title }),
      okButtonProps: { status: 'danger' },
      onOk: async () => {
        const nextCustoms = customs.filter((c) => c.id !== selectedCustom.id);
        try {
          await patchCompanion({ persona: { selected: 'lively', customs: nextCustoms } });
        } catch (e) {
          Message.error(String(e));
        }
      },
    });
  }, [selectedCustom, customs, patchCompanion, t]);

  return (
    <NomiSettingSection
      title={t('nomi.overview.personaSection', { defaultValue: '伙伴设定' })}
      description={t('nomi.overview.personaSectionHint', { defaultValue: '它是谁、怎么说话，都会写进每次对话的开场' })}
    >
      <NomiSettingList>
        <NomiSettingRow
          title={t('nomi.overview.personaTitle', { defaultValue: '角色介绍' })}
          description={t('nomi.settings.personaHint', {
            defaultValue: '决定 {{companionName}} 说话的性格与语气',
            companionName,
          })}
          controls={
            <div className='flex items-center gap-8px'>
              <NomiSelect
                contentFit
                contentMaxWidth={260}
                value={selected}
                onChange={(next: string) => {
                  if (next === selected) return;
                  void patchCompanion({ persona: { selected: next } }).catch((e) => Message.error(String(e)));
                }}
              >
                {BUILTIN_PERSONAS.map((key) => (
                  <NomiSelect.Option key={key} value={key}>
                    {builtinLabel(key)}
                  </NomiSelect.Option>
                ))}
                {customs.map((c) => (
                  <NomiSelect.Option key={c.id} value={c.id}>
                    {c.title.trim() || t('nomi.settings.personaCustomUntitled')}
                  </NomiSelect.Option>
                ))}
              </NomiSelect>
              <Button
                size='small'
                type='outline'
                disabled={customs.length >= MAX_CUSTOM_PERSONAS}
                onClick={() => {
                  setCreateTitle('');
                  setCreateBody('');
                  setCreateOpen(true);
                }}
              >
                {t('nomi.settings.personaCustomCreate')}
              </Button>
            </div>
          }
          footer={
            selectedCustom ? (
              <div className='flex flex-col gap-8px'>
                <Input.TextArea
                  autoSize={{ minRows: 2, maxRows: 6 }}
                  maxLength={MAX_BODY_CHARS}
                  className='!bg-[var(--color-bg-1)] !border-[var(--color-border-2)] !rd-8px !px-10px !py-7px !leading-20px'
                  placeholder={t('nomi.settings.personaCustomBodyPlaceholder')}
                  value={bodyDraft}
                  onChange={onBodyChange}
                />
                <div>
                  <Button size='mini' status='danger' onClick={deleteSelectedCustom}>
                    {t('nomi.settings.personaCustomDelete')}
                  </Button>
                </div>
              </div>
            ) : undefined
          }
        />

        {profile.applied_preset ? (
          <NomiSettingRow
            title={t('nomi.settings.appliedPreset', { defaultValue: '已应用设定' })}
            description={t('nomi.settings.appliedPresetHint', {
              defaultValue: '该伙伴继续沿用已保存的设定快照；新的对话可在首页选择设定。',
            })}
            controls={
              <span className='max-w-240px truncate text-13px text-t-secondary' title={profile.applied_preset.preset_name}>
                {profile.applied_preset.preset_name}
              </span>
            }
          />
        ) : null}
      </NomiSettingList>

      <Modal
        title={t('nomi.settings.personaCustomCreateTitle')}
        visible={createOpen}
        onCancel={() => setCreateOpen(false)}
        onOk={() => void submitCreate()}
        confirmLoading={creating}
        okText={t('nomi.settings.personaCustomCreateConfirm')}
        unmountOnExit
      >
        <div className='flex flex-col gap-12px'>
          <div>
            <div className='text-13px text-t-secondary mb-4px'>{t('nomi.settings.personaCustomTitle')}</div>
            <Input
              maxLength={MAX_TITLE_CHARS}
              placeholder={t('nomi.settings.personaCustomTitlePlaceholder')}
              value={createTitle}
              onChange={setCreateTitle}
            />
          </div>
          <div>
            <div className='text-13px text-t-secondary mb-4px'>{t('nomi.settings.personaCustomBody')}</div>
            <Input.TextArea
              rows={4}
              maxLength={MAX_BODY_CHARS}
              placeholder={t('nomi.settings.personaCustomBodyPlaceholder')}
              value={createBody}
              onChange={setCreateBody}
            />
          </div>
        </div>
      </Modal>
    </NomiSettingSection>
  );
};

export default PersonaSection;
