import React, { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, Spin } from '@arco-design/web-react';
import { Delete, Plus, Peoples } from '@icon-park/react';
import { deleteCameo, listCameos, loadCameoPreviewUrl, uploadCameo } from '../api';
import type { CameoPhoto } from '../types';
import { isSupportedImageFile } from '../home/documentUpload';

export interface WorkspaceCameoStripProps {
  sessionId: string;
  disabled?: boolean;
  /** Bump to force a re-fetch (e.g. after home launch upload / plan). */
  refreshToken?: number | string;
  onChanged?: () => void;
}

const ACCEPT = 'image/png,image/jpeg,image/jpg,image/webp,.png,.jpg,.jpeg,.webp';
const MAX_REFS = 8;

const WorkspaceCameoStrip: React.FC<WorkspaceCameoStripProps> = ({
  sessionId,
  disabled,
  refreshToken,
  onChanged,
}) => {
  const { t } = useTranslation();
  const inputRef = useRef<HTMLInputElement | null>(null);
  const [photos, setPhotos] = useState<CameoPhoto[]>([]);
  const [previews, setPreviews] = useState<Record<string, string>>({});
  const [loading, setLoading] = useState(true);
  const [listError, setListError] = useState<string | null>(null);
  const [deletingId, setDeletingId] = useState<string | null>(null);
  const [uploading, setUploading] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const list = await listCameos(sessionId);
      setPhotos(list);
      setListError(null);
      const next: Record<string, string> = {};
      await Promise.all(
        list.map(async (p) => {
          try {
            next[p.id] = await loadCameoPreviewUrl(sessionId, p.id);
          } catch {
            // ignore preview failures
          }
        })
      );
      setPreviews((prev) => {
        for (const url of Object.values(prev)) {
          if (url.startsWith('blob:')) {
            try {
              URL.revokeObjectURL(url);
            } catch {
              // ignore
            }
          }
        }
        return next;
      });
    } catch (e) {
      setPhotos([]);
      setListError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, [sessionId]);

  useEffect(() => {
    void refresh();
    return () => {
      setPreviews((prev) => {
        for (const url of Object.values(prev)) {
          if (url.startsWith('blob:')) {
            try {
              URL.revokeObjectURL(url);
            } catch {
              // ignore
            }
          }
        }
        return {};
      });
    };
  }, [refresh, refreshToken]);

  const onDelete = async (id: string) => {
    if (disabled) return;
    setDeletingId(id);
    try {
      await deleteCameo(sessionId, id);
      await refresh();
      onChanged?.();
    } finally {
      setDeletingId(null);
    }
  };

  const onPickFiles = async (files: FileList | null) => {
    if (disabled || uploading || !files?.length) return;
    const images = Array.from(files).filter(isSupportedImageFile);
    if (images.length === 0) return;
    const room = Math.max(0, MAX_REFS - photos.length);
    const slice = images.slice(0, room);
    if (slice.length === 0) return;
    setUploading(true);
    try {
      for (const [index, file] of slice.entries()) {
        const label =
          file.name.replace(/\.[^.]+$/, '').trim().slice(0, 48) ||
          `参考图${photos.length + index + 1}`;
        await uploadCameo(sessionId, file, label, '');
      }
      await refresh();
      onChanged?.();
    } catch (e) {
      setListError(e instanceof Error ? e.message : String(e));
    } finally {
      setUploading(false);
      if (inputRef.current) inputRef.current.value = '';
    }
  };

  return (
    <div className='mt-12px flex flex-col gap-8px rd-10px border border-solid border-[var(--color-border-2)] bg-[var(--color-fill-1)] p-12px'>
      <div className='flex flex-wrap items-center justify-between gap-8px'>
        <div className='inline-flex items-center gap-6px text-12px font-650 text-[var(--color-text-2)]'>
          <Peoples theme='outline' size={14} />
          {t('videoGeneration.workspace.cameo.title', { defaultValue: '参考图' })}
          {photos.length > 0 ? (
            <span className='font-400 text-[var(--color-text-3)]'>({photos.length})</span>
          ) : null}
        </div>
        <Button
          type='outline'
          size='mini'
          loading={uploading}
          disabled={disabled || uploading || photos.length >= MAX_REFS}
          onClick={() => inputRef.current?.click()}
        >
          <Plus theme='outline' size={12} />
          {t('videoGeneration.workspace.cameo.add', { defaultValue: '添加' })}
        </Button>
        <input
          ref={inputRef}
          type='file'
          accept={ACCEPT}
          multiple
          className='hidden'
          onChange={(e) => void onPickFiles(e.target.files)}
        />
      </div>
      {loading ? (
        <div className='flex justify-center py-10px'>
          <Spin size={18} />
        </div>
      ) : listError ? (
        <p className='m-0 text-12px text-danger-6'>
          {t('videoGeneration.workspace.cameo.loadFailed', {
            defaultValue: '参考图列表加载失败：{{error}}',
            error: listError,
          })}
        </p>
      ) : photos.length === 0 ? (
        <p className='m-0 text-12px text-[var(--color-text-3)]'>
          {t('videoGeneration.workspace.cameo.empty', {
            defaultValue:
              '当前任务还没有参考图。可点右上角「添加」，或返回首页重新选择图片后发送。规划完成后也会按类型出现在「技术产物 → 参考图」。',
          })}
        </p>
      ) : (
        <div className='flex flex-wrap gap-8px'>
          {photos.map((photo) => (
            <div
              key={photo.id}
              className='flex items-center gap-8px rd-8px bg-[var(--color-bg-2)] px-8px py-6px'
            >
              <div className='h-40px w-40px overflow-hidden rd-6px bg-[var(--color-fill-2)]'>
                {previews[photo.id] ? (
                  <img
                    src={previews[photo.id]}
                    alt={photo.character_name}
                    className='h-full w-full object-cover'
                  />
                ) : null}
              </div>
              <div className='min-w-0'>
                <div className='truncate text-12px font-600 text-[var(--color-text-1)]'>
                  {photo.character_name}
                </div>
                {photo.bound_identifier ? (
                  <div className='truncate text-11px text-[var(--color-text-3)]'>
                    → {photo.bound_identifier}
                  </div>
                ) : null}
              </div>
              <Button
                type='text'
                size='mini'
                status='danger'
                loading={deletingId === photo.id}
                disabled={disabled || !!deletingId}
                onClick={() => void onDelete(photo.id)}
                aria-label={t('videoGeneration.workspace.cameo.delete', { defaultValue: '删除' })}
              >
                <Delete theme='outline' size={13} />
              </Button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
};

export default WorkspaceCameoStrip;
