import React, { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Spin } from '@arco-design/web-react';
import { People, VideoOne } from '@icon-park/react';
import {
  listActionAssets,
  loadArtifactMediaUrl,
  uploadActionAssets,
} from '../api';
import type { ActionAssetsInfo } from '../types';
import {
  ACTION_CHARACTER_ACCEPT,
  ACTION_CHARACTER_MAX_BYTES,
  ACTION_VIDEO_ACCEPT,
  ACTION_VIDEO_MAX_BYTES,
  isSupportedImageFile,
  isSupportedVideoFile,
} from '../home/documentUpload';

export interface WorkspaceActionAssetsProps {
  sessionId: string;
  disabled?: boolean;
  onReadyChange?: (ready: boolean) => void;
}

function isReady(info: ActionAssetsInfo | null): boolean {
  return Boolean(info?.character && info?.reference_video);
}

const WorkspaceActionAssets: React.FC<WorkspaceActionAssetsProps> = ({
  sessionId,
  disabled,
  onReadyChange,
}) => {
  const { t } = useTranslation();
  const characterInputRef = useRef<HTMLInputElement | null>(null);
  const videoInputRef = useRef<HTMLInputElement | null>(null);
  const [info, setInfo] = useState<ActionAssetsInfo | null>(null);
  const [characterUrl, setCharacterUrl] = useState<string | null>(null);
  const [videoUrl, setVideoUrl] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [uploading, setUploading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const applyInfo = useCallback(
    async (next: ActionAssetsInfo) => {
      setInfo(next);
      onReadyChange?.(isReady(next));
      const nextCharacter = next.character
        ? await loadArtifactMediaUrl(sessionId, next.character).catch(() => null)
        : null;
      const nextVideo = next.reference_video
        ? await loadArtifactMediaUrl(sessionId, next.reference_video).catch(() => null)
        : null;
      setCharacterUrl((prev) => {
        if (prev?.startsWith('blob:')) URL.revokeObjectURL(prev);
        return nextCharacter;
      });
      setVideoUrl((prev) => {
        if (prev?.startsWith('blob:')) URL.revokeObjectURL(prev);
        return nextVideo;
      });
    },
    [onReadyChange, sessionId]
  );

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const next = await listActionAssets(sessionId);
      await applyInfo(next);
      setError(null);
    } catch (e) {
      setInfo(null);
      onReadyChange?.(false);
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, [applyInfo, onReadyChange, sessionId]);

  useEffect(() => {
    void refresh();
    return () => {
      setCharacterUrl((prev) => {
        if (prev?.startsWith('blob:')) URL.revokeObjectURL(prev);
        return null;
      });
      setVideoUrl((prev) => {
        if (prev?.startsWith('blob:')) URL.revokeObjectURL(prev);
        return null;
      });
    };
  }, [refresh]);

  const upload = async (kind: 'character' | 'video', file: File) => {
    if (disabled || uploading) return;
    if (kind === 'character') {
      if (!isSupportedImageFile(file)) {
        setError(
          t('videoGeneration.create.action.unsupported', {
            defaultValue: '请上传 PNG / JPEG / WEBP 角色图，或 MP4 / WebM / MOV 参考视频。',
          })
        );
        return;
      }
      if (file.size > ACTION_CHARACTER_MAX_BYTES) {
        setError(
          t('videoGeneration.create.action.characterTooLarge', {
            defaultValue: '角色图不能超过 10 MB。',
          })
        );
        return;
      }
    } else {
      if (!isSupportedVideoFile(file)) {
        setError(
          t('videoGeneration.create.action.unsupported', {
            defaultValue: '请上传 PNG / JPEG / WEBP 角色图，或 MP4 / WebM / MOV 参考视频。',
          })
        );
        return;
      }
      if (file.size > ACTION_VIDEO_MAX_BYTES) {
        setError(
          t('videoGeneration.create.action.videoTooLarge', {
            defaultValue: '参考视频不能超过 80 MB。',
          })
        );
        return;
      }
    }
    setUploading(true);
    setError(null);
    try {
      const next = await uploadActionAssets(sessionId, {
        character: kind === 'character' ? file : undefined,
        video: kind === 'video' ? file : undefined,
      });
      await applyInfo(next);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setUploading(false);
    }
  };

  return (
    <div className='flex flex-col gap-10px'>
      {loading ? (
        <div className='flex justify-center py-16px'>
          <Spin size={18} />
        </div>
      ) : (
        <div className='grid grid-cols-1 gap-10px md:grid-cols-2'>
          <button
            type='button'
            disabled={disabled || uploading}
            onClick={() => characterInputRef.current?.click()}
            className='relative flex min-h-148px flex-col items-center justify-center gap-8px overflow-hidden rd-12px border border-dashed border-[var(--color-border-3)] bg-[var(--color-fill-1)] text-[var(--color-text-3)] disabled:cursor-not-allowed disabled:opacity-55'
          >
            {characterUrl ? (
              <img src={characterUrl} alt='' className='absolute inset-0 size-full object-cover' />
            ) : (
              <People theme='outline' size={22} />
            )}
            <span
              className={[
                'relative z-1 text-12px font-600',
                characterUrl
                  ? 'absolute right-0 bottom-0 left-0 px-10px py-8px text-white'
                  : '',
              ].join(' ')}
              style={
                characterUrl
                  ? { background: 'linear-gradient(transparent, rgba(0,0,0,.55))' }
                  : undefined
              }
            >
              {t('videoGeneration.create.action.character', { defaultValue: '角色图' })}
            </span>
          </button>
          <input
            ref={characterInputRef}
            type='file'
            accept={ACTION_CHARACTER_ACCEPT}
            hidden
            disabled={disabled || uploading}
            onChange={(event) => {
              const file = event.target.files?.[0];
              if (file) void upload('character', file);
              event.target.value = '';
            }}
          />
          <button
            type='button'
            disabled={disabled || uploading}
            onClick={() => videoInputRef.current?.click()}
            className='relative flex min-h-148px flex-col items-center justify-center gap-8px overflow-hidden rd-12px border border-dashed border-[var(--color-border-3)] bg-[var(--color-fill-1)] text-[var(--color-text-3)] disabled:cursor-not-allowed disabled:opacity-55'
          >
            {videoUrl ? (
              <video
                src={videoUrl}
                className='absolute inset-0 size-full object-cover'
                muted
                playsInline
              />
            ) : (
              <VideoOne theme='outline' size={22} />
            )}
            <span
              className={[
                'relative z-1 text-12px font-600',
                videoUrl ? 'absolute right-0 bottom-0 left-0 px-10px py-8px text-white' : '',
              ].join(' ')}
              style={
                videoUrl
                  ? { background: 'linear-gradient(transparent, rgba(0,0,0,.55))' }
                  : undefined
              }
            >
              {t('videoGeneration.create.action.video', { defaultValue: '参考视频' })}
            </span>
          </button>
          <input
            ref={videoInputRef}
            type='file'
            accept={ACTION_VIDEO_ACCEPT}
            hidden
            disabled={disabled || uploading}
            onChange={(event) => {
              const file = event.target.files?.[0];
              if (file) void upload('video', file);
              event.target.value = '';
            }}
          />
        </div>
      )}
      {error ? (
        <p className='m-0 text-12px text-[rgb(var(--danger-6))]'>{error}</p>
      ) : (
        <p className='m-0 text-12px text-[var(--color-text-3)]'>
          {t('videoGeneration.workspace.action.replaceHint', {
            defaultValue: '点击上方区域可替换素材。时长跟随参考视频，画幅由模型自适应。',
          })}
        </p>
      )}
    </div>
  );
};

export default WorkspaceActionAssets;
