import React, { useCallback, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Edit } from '@icon-park/react';
import { CanvasChromeButton } from '@oc/components/canvas/canvas-overlay';
import { SESSION_TITLE_MAX_CHARS } from '../api';
import styles from '../index.module.css';

type Props = {
  title: string;
  disabled?: boolean;
  onSave: (next: string) => Promise<void>;
};

export default function WorkspaceTitleField({ title, disabled, onSave }: Props) {
  const { t } = useTranslation();
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState('');
  const [saving, setSaving] = useState(false);
  const editingRef = useRef(false);
  const untitled = t('videoGeneration.list.untitled', { defaultValue: '未命名任务' });
  const display = title.trim() || untitled;

  const startEditing = useCallback(() => {
    if (disabled || saving) return;
    editingRef.current = true;
    setDraft(title);
    setEditing(true);
  }, [disabled, saving, title]);

  const cancelEditing = useCallback(() => {
    editingRef.current = false;
    setEditing(false);
  }, []);

  const commitEditing = useCallback(async () => {
    if (!editingRef.current) return;
    editingRef.current = false;
    setEditing(false);
    const next = draft.trim().slice(0, SESSION_TITLE_MAX_CHARS);
    if (next === title.trim()) return;
    setSaving(true);
    try {
      await onSave(next);
    } finally {
      setSaving(false);
    }
  }, [draft, onSave, title]);

  if (editing) {
    return (
      <input
        autoFocus
        disabled={saving}
        maxLength={SESSION_TITLE_MAX_CHARS}
        value={draft}
        onChange={(event) => setDraft(event.target.value)}
        onBlur={() => {
          void commitEditing();
        }}
        onKeyDown={(event) => {
          if (event.key === 'Enter') {
            event.preventDefault();
            void commitEditing();
          }
          if (event.key === 'Escape') {
            event.preventDefault();
            cancelEditing();
          }
        }}
        className={styles.studioTitleInput}
        placeholder={t('videoGeneration.workspace.titlePlaceholder', {
          defaultValue: '给这次任务起个名字',
        })}
        aria-label={t('videoGeneration.workspace.rename', { defaultValue: '修改标题' })}
      />
    );
  }

  return (
    <div className={styles.studioTitleRow}>
      <button
        type='button'
        className={styles.studioTitleButton}
        onClick={startEditing}
        disabled={disabled || saving}
        title={t('videoGeneration.workspace.renameHint', { defaultValue: '点击修改任务标题' })}
      >
        {display}
      </button>
      <CanvasChromeButton
        className='is-icon shrink-0 opacity-60 hover:opacity-100'
        onClick={startEditing}
        disabled={disabled || saving}
        title={t('videoGeneration.workspace.rename', { defaultValue: '修改标题' })}
        aria-label={t('videoGeneration.workspace.rename', { defaultValue: '修改标题' })}
      >
        <Edit theme='outline' size={14} fill='currentColor' />
      </CanvasChromeButton>
    </div>
  );
}
