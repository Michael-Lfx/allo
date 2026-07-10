/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Message, Modal } from '@arco-design/web-react';
import { CheckOne, EditTwo, Plus } from '@icon-park/react';
import classNames from 'classnames';
import { ThemeSwitcher } from '@renderer/components/settings/ThemeSwitcher';
import FontSizeControl from '@renderer/components/settings/FontSizeControl';
import CssThemeModal from '@renderer/pages/settings/DisplaySettings/CssThemeModal';
import { getCssThemeDisplayName } from '@renderer/pages/settings/DisplaySettings/presets';
import { useCssTheme } from '@renderer/hooks/ui/useCssTheme';
import type { ICssTheme } from '@/common/config/storage';

/** Pull a representative accent color out of a preset's CSS for the swatch dot. */
const pickAccent = (css: string): string | null => {
  const match = css.match(/--(?:color-primary|primary-6)\s*:\s*([^;!}]+)/i);
  if (!match) return null;
  const value = match[1].trim().replace(/\s*!important\s*/i, '');
  if (!value || /var\(/i.test(value)) return null;
  if (/^\d{1,3}\s*,\s*\d{1,3}\s*,\s*\d{1,3}$/.test(value)) return `rgb(${value})`;
  return value;
};

interface SiderThemePanelProps {
  className?: string;
  onBeforeOpenModal?: () => void;
}

/**
 * Shared theme/skin panel: light–dark, font size, and CSS presets.
 * Used by the footer theme button and the user menu "换肤" section.
 */
const SiderThemePanel: React.FC<SiderThemePanelProps> = ({ className, onBeforeOpenModal }) => {
  const { t } = useTranslation();
  const { themes, activeThemeId, selectTheme, saveUserTheme, deleteUserTheme } = useCssTheme();
  const [modalVisible, setModalVisible] = useState(false);
  const [editingTheme, setEditingTheme] = useState<ICssTheme | null>(null);

  const openModal = (theme: ICssTheme | null) => {
    onBeforeOpenModal?.();
    setEditingTheme(theme);
    setModalVisible(true);
  };

  const closeModal = () => {
    setModalVisible(false);
    setEditingTheme(null);
  };

  const handleSave = async (data: Omit<ICssTheme, 'id' | 'created_at' | 'updated_at' | 'is_preset'>) => {
    await saveUserTheme(data, editingTheme);
    closeModal();
    Message.success(t('common.saveSuccess'));
  };

  const canDelete = !!editingTheme && !editingTheme.is_preset;
  const handleDelete = () => {
    if (!editingTheme || editingTheme.is_preset) return;
    const target = editingTheme;
    Modal.confirm({
      title: t('common.confirmDelete'),
      content: t('settings.cssTheme.deleteConfirm'),
      okButtonProps: { status: 'danger' },
      onOk: async () => {
        await deleteUserTheme(target.id);
        closeModal();
        Message.success(t('common.deleteSuccess'));
      },
    });
  };

  return (
    <>
      <div className={classNames('w-full flex flex-col gap-10px p-8px', className)}>
        <div className='flex flex-col gap-4px'>
          <div className='text-11px font-500 text-t-tertiary px-2px'>{t('settings.theme')}</div>
          <ThemeSwitcher />
        </div>

        <div className='flex flex-col gap-4px'>
          <div className='text-11px font-500 text-t-tertiary px-2px'>{t('settings.fontSize')}</div>
          <FontSizeControl />
        </div>

        <div className='flex flex-col gap-4px'>
          <div className='text-11px font-500 text-t-tertiary px-2px'>{t('settings.cssTheme.selectOrCustomize')}</div>
          <div className='flex flex-col gap-1px max-h-180px overflow-y-auto -mx-2px px-2px'>
            {themes.map((theme) => {
              const active = activeThemeId === theme.id;
              const accent = pickAccent(theme.css || '');
              const displayName = getCssThemeDisplayName(theme, t);
              return (
                <div
                  key={theme.id}
                  className={classNames(
                    'group flex items-center gap-6px h-28px px-6px rd-6px text-left transition-colors',
                    active ? '!bg-primary-1' : 'hover:bg-fill-2'
                  )}
                >
                  <button
                    type='button'
                    onClick={() => void selectTheme(theme)}
                    className='flex-1 min-w-0 flex items-center gap-6px cursor-pointer border-none bg-transparent p-0 text-left'
                  >
                    <span
                      className='size-12px rd-full shrink-0 border border-solid border-[var(--color-border-2)]'
                      style={accent ? { background: accent } : { background: 'var(--color-fill-3)' }}
                    />
                    <span
                      className={classNames(
                        'flex-1 min-w-0 truncate text-12px',
                        active ? 'text-primary-6 font-500' : 'text-t-primary'
                      )}
                    >
                      {displayName}
                    </span>
                  </button>
                  {active && <CheckOne theme='filled' size='14' fill='rgb(var(--primary-6))' className='shrink-0' />}
                  <button
                    type='button'
                    onClick={() => openModal(theme)}
                    aria-label={t('settings.cssTheme.editTheme')}
                    className='shrink-0 opacity-0 group-hover:opacity-100 size-20px flex items-center justify-center rd-4px text-t-tertiary hover:text-primary-6 hover:bg-fill-3 cursor-pointer border-none bg-transparent transition-opacity'
                  >
                    <EditTwo theme='outline' size='12' fill='currentColor' />
                  </button>
                </div>
              );
            })}

            <button
              type='button'
              onClick={() => openModal(null)}
              className='flex items-center gap-6px h-28px px-6px rd-6px text-12px text-t-secondary hover:text-primary-6 hover:bg-fill-2 cursor-pointer border-none bg-transparent transition-colors'
            >
              <span className='size-12px shrink-0 flex items-center justify-center'>
                <Plus theme='outline' size='12' fill='currentColor' />
              </span>
              <span className='flex-1 min-w-0 truncate text-left'>{t('settings.cssTheme.addManually')}</span>
            </button>
          </div>
        </div>
      </div>

      <CssThemeModal
        visible={modalVisible}
        theme={editingTheme}
        onClose={closeModal}
        onSave={(data) => void handleSave(data)}
        onDelete={canDelete ? handleDelete : undefined}
      />
    </>
  );
};

export default SiderThemePanel;
