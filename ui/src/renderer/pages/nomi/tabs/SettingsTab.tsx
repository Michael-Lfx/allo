/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, Input, Message, Modal, Radio, Spin, TimePicker } from '@arco-design/web-react';
import { ipcBridge } from '@/common';
import type { ICustomPersona } from '@/common/adapter/ipcBridge';
import { CUSTOM_CHARACTER_ID } from '@renderer/pages/companion/characters';
import { customFigureMetaOf } from '@renderer/pages/companion/characters/customMeta';
import CharacterPicker from '../CharacterPicker';
import { figureToCustomPatch } from '../useFigures';
import type { useCompanion } from '../useNomi';

interface Props {
  companion: ReturnType<typeof useCompanion>;
  /** Called after this companion was deleted so the page can switch selection. */
  onDeleted: (companionId: string) => void;
}

const BUILTIN_PERSONAS = ['lively', 'calm', 'sassy'] as const;
const MAX_CUSTOM_PERSONAS = 10;
const MAX_TITLE_CHARS = 20;
const MAX_BODY_CHARS = 2000;

/**
 * Debounced text editing over an optimistically-patched source value: local
 * draft follows keystrokes, the commit fires after `delay` ms of quiet.
 */
const useDebouncedText = (source: string, commit: (value: string) => void, delay = 500) => {
  const [draft, setDraft] = useState(source);
  const timerRef = useRef<number | undefined>(undefined);
  const commitRef = useRef(commit);
  commitRef.current = commit;

  useEffect(() => {
    setDraft(source);
  }, [source]);
  useEffect(() => () => window.clearTimeout(timerRef.current), []);

  const onChange = useCallback(
    (value: string) => {
      setDraft(value);
      window.clearTimeout(timerRef.current);
      timerRef.current = window.setTimeout(() => commitRef.current(value), delay);
    },
    [delay]
  );

  return [draft, onChange] as const;
};

const SettingsTab: React.FC<Props> = ({ companion, onDeleted }) => {
  const { t } = useTranslation();
  const { profile, patchCompanion } = companion;

  const [nameDraft, onNameChange] = useDebouncedText(profile?.name ?? '', (value) => {
    const name = value.trim();
    if (!name || name === profile?.name) return;
    void patchCompanion({ name }).catch((e) => Message.error(String(e)));
  });

  const [createOpen, setCreateOpen] = useState(false);
  const [createTitle, setCreateTitle] = useState('');
  const [createBody, setCreateBody] = useState('');
  const [creating, setCreating] = useState(false);

  const customs = profile?.persona.customs ?? [];
  const selected = profile?.persona.selected ?? 'lively';
  const selectedCustom = useMemo(
    () => customs.find((c) => c.id === selected) ?? null,
    [customs, selected]
  );
  const isBuiltinSelected = (BUILTIN_PERSONAS as readonly string[]).includes(selected);

  const [titleDraft, onTitleChange] = useDebouncedText(selectedCustom?.title ?? '', (title) => {
    if (!profile || !selectedCustom) return;
    const trimmed = title.trim();
    if (!trimmed || trimmed === selectedCustom.title) return;
    const nextCustoms = customs.map((c) => (c.id === selectedCustom.id ? { ...c, title: trimmed } : c));
    void patchCompanion({ persona: { customs: nextCustoms } }).catch((e) => Message.error(String(e)));
  });

  const [bodyDraft, onBodyChange] = useDebouncedText(selectedCustom?.body ?? '', (body) => {
    if (!profile || !selectedCustom) return;
    const trimmed = body.trim();
    if (!trimmed || trimmed === selectedCustom.body) return;
    const nextCustoms = customs.map((c) => (c.id === selectedCustom.id ? { ...c, body: trimmed } : c));
    void patchCompanion({ persona: { customs: nextCustoms } }).catch((e) => Message.error(String(e)));
  });

  const confirmDelete = useCallback(() => {
    if (!profile) return;
    const companionName = profile.name;
    Modal.confirm({
      title: t('nomi.settings.deleteConfirmTitle'),
      content: t('nomi.settings.deleteConfirmBody', { companionName }),
      okButtonProps: { status: 'danger' },
      onOk: async () => {
        try {
          await ipcBridge.companion.deleteCompanion.invoke({ companion_id: profile.id });
          Message.success(t('nomi.settings.deleted', { companionName }));
          onDeleted(profile.id);
        } catch (e) {
          Message.error(String(e));
        }
      },
    });
  }, [profile, onDeleted, t]);

  const selectPersona = useCallback(
    (value: string) => {
      if (!profile || value === profile.persona.selected) return;
      void patchCompanion({ persona: { selected: value } }).catch((e) => Message.error(String(e)));
    },
    [profile, patchCompanion]
  );

  const openCreate = useCallback(() => {
    if (customs.length >= MAX_CUSTOM_PERSONAS) {
      Message.warning(t('nomi.settings.personaCustomLimit', { max: MAX_CUSTOM_PERSONAS }));
      return;
    }
    setCreateTitle('');
    setCreateBody('');
    setCreateOpen(true);
  }, [customs.length, t]);

  const submitCreate = useCallback(async () => {
    if (!profile) return;
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
      await patchCompanion({
        persona: {
          selected: entry.id,
          customs: [...customs, entry],
        },
      });
      setCreateOpen(false);
    } catch (e) {
      Message.error(String(e));
    } finally {
      setCreating(false);
    }
  }, [profile, createTitle, createBody, customs, patchCompanion, t]);

  const deleteSelectedCustom = useCallback(() => {
    if (!profile || !selectedCustom) return;
    Modal.confirm({
      title: t('nomi.settings.personaCustomDeleteTitle'),
      content: t('nomi.settings.personaCustomDeleteBody', { title: selectedCustom.title }),
      okButtonProps: { status: 'danger' },
      onOk: async () => {
        const nextCustoms = customs.filter((c) => c.id !== selectedCustom.id);
        const nextSelected = selected === selectedCustom.id ? 'lively' : selected;
        try {
          await patchCompanion({
            persona: { selected: nextSelected, customs: nextCustoms },
          });
        } catch (e) {
          Message.error(String(e));
        }
      },
    });
  }, [profile, selectedCustom, customs, selected, patchCompanion, t]);

  if (!profile) {
    return (
      <div className='flex justify-center py-40px'>
        <Spin />
      </div>
    );
  }

  const companionName = profile.name;
  const atCustomLimit = customs.length >= MAX_CUSTOM_PERSONAS;

  const row = (label: string, hint: string | null, control: React.ReactNode) => (
    <div className='flex items-start gap-16px bg-fill-2 rd-10px px-14px py-12px'>
      <div className='w-200px shrink-0'>
        <div className='text-14px text-t-primary font-500'>{label}</div>
        {hint && <div className='text-12px text-t-tertiary mt-2px'>{hint}</div>}
      </div>
      <div className='flex-1 min-w-0'>{control}</div>
    </div>
  );

  const builtinLabel = (key: (typeof BUILTIN_PERSONAS)[number]) => {
    if (key === 'lively') return t('nomi.settings.personaLively');
    if (key === 'calm') return t('nomi.settings.personaCalm');
    return t('nomi.settings.personaSassy');
  };

  return (
    <div className='flex flex-col gap-10px py-8px'>
      {row(
        t('nomi.settings.name'),
        t('nomi.settings.nameHint'),
        <Input style={{ width: 260 }} value={nameDraft} onChange={onNameChange} maxLength={30} />
      )}
      {row(
        t('nomi.settings.character'),
        t('nomi.settings.characterHint'),
        <CharacterPicker
          value={profile.character || 'mochi'}
          figureId={customFigureMetaOf(profile)?.figureId}
          onSelectCharacter={(character) => void patchCompanion({ character, appearance: { custom_figure: null } })}
          onSelectFigure={(fig) =>
            void patchCompanion({
              character: CUSTOM_CHARACTER_ID,
              appearance: { custom_figure: figureToCustomPatch(fig) },
            })
          }
        />
      )}
      {row(
        t('nomi.settings.persona'),
        t('nomi.settings.personaHint', { companionName }),
        <div className='flex flex-col gap-8px'>
          <div className='flex flex-wrap items-center gap-8px'>
            <Radio.Group type='button' value={selected} onChange={selectPersona}>
              {BUILTIN_PERSONAS.map((key) => (
                <Radio key={key} value={key}>
                  {builtinLabel(key)}
                </Radio>
              ))}
              {customs.map((c) => (
                <Radio key={c.id} value={c.id}>
                  {c.title.trim() || t('nomi.settings.personaCustomUntitled')}
                </Radio>
              ))}
            </Radio.Group>
            <Button size='small' type='outline' disabled={atCustomLimit} onClick={openCreate}>
              {t('nomi.settings.personaCustomCreate')}
            </Button>
          </div>
          {!isBuiltinSelected && selectedCustom && (
            <div className='flex flex-col gap-8px'>
              <Input
                style={{ maxWidth: 320 }}
                maxLength={MAX_TITLE_CHARS}
                placeholder={t('nomi.settings.personaCustomTitlePlaceholder')}
                value={titleDraft}
                onChange={onTitleChange}
              />
              <Input.TextArea
                rows={3}
                maxLength={MAX_BODY_CHARS}
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
          )}
        </div>
      )}
      {row(
        t('nomi.settings.quietHours'),
        t('nomi.settings.quietHoursHint'),
        <TimePicker.RangePicker
          format='HH:mm'
          allowClear
          value={
            profile.appearance.quiet_start && profile.appearance.quiet_end
              ? [profile.appearance.quiet_start, profile.appearance.quiet_end]
              : undefined
          }
          onChange={(value) => {
            const [quiet_start, quiet_end] = (value as string[] | undefined) ?? ['', ''];
            void patchCompanion({
              appearance: { quiet_start: quiet_start || '', quiet_end: quiet_end || '' },
            });
          }}
        />
      )}

      <div className='mt-8px text-13px font-600 text-[rgb(var(--danger-6))]'>{t('nomi.settings.danger')}</div>
      {row(
        t('nomi.settings.deleteCompanion'),
        t('nomi.settings.deleteCompanionHint', { companionName }),
        <Button status='danger' onClick={confirmDelete}>
          {t('nomi.settings.deleteCompanion')}
        </Button>
      )}

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
    </div>
  );
};

export default SettingsTab;
