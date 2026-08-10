import React, { useCallback, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, Input } from '@arco-design/web-react';
import { Delete, Plus, Peoples } from '@icon-park/react';
import type { CameoDraftItem } from '../types';
import { suggestCameoCharacterName } from '../cameoUtils';

const ACCEPT = 'image/png,image/jpeg,image/jpg,image/webp';
const MAX_CAMEOS = 8;

export interface CameoCastEditorProps {
  value: CameoDraftItem[];
  onChange: (next: CameoDraftItem[]) => void;
  disabled?: boolean;
}

function newLocalId(): string {
  return `local_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 8)}`;
}

const CameoCastEditor: React.FC<CameoCastEditorProps> = ({ value, onChange, disabled }) => {
  const { t } = useTranslation();
  const inputRef = useRef<HTMLInputElement | null>(null);

  const revokePreview = useCallback((item: CameoDraftItem) => {
    if (item.previewUrl) {
      try {
        URL.revokeObjectURL(item.previewUrl);
      } catch {
        // ignore
      }
    }
  }, []);

  const addFiles = useCallback(
    (files: FileList | File[]) => {
      const list = Array.from(files).filter((f) => /^image\/(png|jpeg|jpg|webp)$/i.test(f.type));
      if (list.length === 0) return;
      const room = Math.max(0, MAX_CAMEOS - value.length);
      const slice = list.slice(0, room);
      if (slice.length === 0) return;
      const added: CameoDraftItem[] = slice.map((file, i) => ({
        localId: newLocalId(),
        characterName: suggestCameoCharacterName(file.name, value.length + i),
        description: '',
        file,
        previewUrl: URL.createObjectURL(file),
      }));
      onChange([...value, ...added]);
    },
    [onChange, value]
  );

  const removeAt = (localId: string) => {
    const target = value.find((c) => c.localId === localId);
    if (target) revokePreview(target);
    onChange(value.filter((c) => c.localId !== localId));
  };

  const patch = (localId: string, patchValue: Partial<CameoDraftItem>) => {
    onChange(value.map((c) => (c.localId === localId ? { ...c, ...patchValue } : c)));
  };

  return (
    <div className='mt-12px flex flex-col gap-10px rd-12px border border-solid border-[var(--color-border-2)] bg-[var(--color-fill-1)] p-12px text-left'>
      <div className='flex flex-wrap items-start justify-between gap-8px'>
        <div>
          <div className='inline-flex items-center gap-6px text-13px font-650 text-[var(--color-text-1)]'>
            <Peoples theme='outline' size={15} />
            {t('videoGeneration.create.cameo.title', { defaultValue: '角色参考图（Cameo）' })}
          </div>
          <p className='m-0 mt-4px text-12px leading-18px text-[var(--color-text-3)]'>
            {t('videoGeneration.create.cameo.hint', {
              defaultValue: '可选：上传人物照片并填写角色名，成片会尽量保持外貌一致。',
            })}
          </p>
        </div>
        <Button
          type='outline'
          size='small'
          disabled={disabled || value.length >= MAX_CAMEOS}
          onClick={() => inputRef.current?.click()}
        >
          <span className='inline-flex items-center gap-4px'>
            <Plus theme='outline' size={14} />
            {t('videoGeneration.create.cameo.add', { defaultValue: '添加照片' })}
          </span>
        </Button>
        <input
          ref={inputRef}
          type='file'
          accept={ACCEPT}
          multiple
          hidden
          disabled={disabled}
          onChange={(e) => {
            if (e.target.files) addFiles(e.target.files);
            e.target.value = '';
          }}
        />
      </div>

      {value.length === 0 ? (
        <button
          type='button'
          disabled={disabled}
          className='flex min-h-72px w-full cursor-pointer flex-col items-center justify-center gap-4px rd-10px border border-dashed border-[var(--color-border-2)] bg-[var(--color-bg-2)] px-12px py-14px text-12px text-[var(--color-text-3)]'
          onClick={() => inputRef.current?.click()}
          onDragOver={(e) => {
            e.preventDefault();
          }}
          onDrop={(e) => {
            e.preventDefault();
            if (disabled) return;
            if (e.dataTransfer.files?.length) addFiles(e.dataTransfer.files);
          }}
        >
          {t('videoGeneration.create.cameo.dropHint', {
            defaultValue: '拖拽或点击上传 PNG / JPEG / WEBP（最多 8 张）',
          })}
        </button>
      ) : (
        <div className='flex flex-col gap-8px'>
          {value.map((item) => (
            <div
              key={item.localId}
              className='flex flex-wrap items-start gap-10px rd-10px bg-[var(--color-bg-2)] p-10px'
            >
              <div className='h-64px w-64px shrink-0 overflow-hidden rd-8px bg-[var(--color-fill-2)]'>
                {item.previewUrl ? (
                  <img
                    src={item.previewUrl}
                    alt={item.characterName}
                    className='h-full w-full object-cover'
                  />
                ) : null}
              </div>
              <div className='min-w-180px flex flex-1 flex-col gap-6px'>
                <Input
                  size='small'
                  value={item.characterName}
                  disabled={disabled}
                  placeholder={t('videoGeneration.create.cameo.namePlaceholder', {
                    defaultValue: '角色名（请改成故事里的名字，如「小」）',
                  })}
                  onChange={(characterName) => patch(item.localId, { characterName })}
                />
                <Input
                  size='small'
                  value={item.description}
                  disabled={disabled}
                  placeholder={t('videoGeneration.create.cameo.descPlaceholder', {
                    defaultValue: '可选描述（身份/服装提示）',
                  })}
                  onChange={(description) => patch(item.localId, { description })}
                />
              </div>
              <Button
                type='text'
                size='mini'
                status='danger'
                disabled={disabled}
                onClick={() => removeAt(item.localId)}
                aria-label={t('videoGeneration.create.cameo.remove', { defaultValue: '移除' })}
              >
                <Delete theme='outline' size={14} />
              </Button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
};

export default CameoCastEditor;
