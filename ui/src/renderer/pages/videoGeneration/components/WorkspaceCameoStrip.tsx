import React, { useCallback, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, Spin } from '@arco-design/web-react';
import { Delete, Peoples } from '@icon-park/react';
import { deleteCameo, listCameos, loadCameoPreviewUrl } from '../api';
import type { CameoPhoto } from '../types';

export interface WorkspaceCameoStripProps {
  sessionId: string;
  disabled?: boolean;
  onChanged?: () => void;
}

const WorkspaceCameoStrip: React.FC<WorkspaceCameoStripProps> = ({
  sessionId,
  disabled,
  onChanged,
}) => {
  const { t } = useTranslation();
  const [photos, setPhotos] = useState<CameoPhoto[]>([]);
  const [previews, setPreviews] = useState<Record<string, string>>({});
  const [loading, setLoading] = useState(true);
  const [deletingId, setDeletingId] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const list = await listCameos(sessionId);
      setPhotos(list);
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
    } catch {
      setPhotos([]);
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
  }, [refresh]);

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

  return (
    <div className='mt-12px flex flex-col gap-8px rd-10px border border-solid border-[var(--color-border-2)] bg-[var(--color-fill-1)] p-12px'>
      <div className='inline-flex items-center gap-6px text-12px font-650 text-[var(--color-text-2)]'>
        <Peoples theme='outline' size={14} />
        {t('videoGeneration.workspace.cameo.title', { defaultValue: '已绑定角色参考图' })}
      </div>
      {loading ? (
        <div className='flex justify-center py-10px'>
          <Spin size={18} />
        </div>
      ) : photos.length === 0 ? (
        <p className='m-0 text-12px text-[var(--color-text-3)]'>
          {t('videoGeneration.workspace.cameo.empty', {
            defaultValue: '暂无 Cameo。可在创建时上传，或重新规划前通过 API/导入添加。',
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
