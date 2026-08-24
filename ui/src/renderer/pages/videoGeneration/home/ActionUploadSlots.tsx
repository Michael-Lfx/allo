import { useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { CloseSmall, People, VideoOne } from '@icon-park/react';
import {
  ACTION_CHARACTER_ACCEPT,
  ACTION_VIDEO_ACCEPT,
} from './documentUpload';
import type { ActionAssetDraft } from './types';
import styles from './home.module.css';

export interface ActionUploadSlotsProps {
  loading: boolean;
  actionCharacter: ActionAssetDraft | null;
  actionVideo: ActionAssetDraft | null;
  setActionCharacter: (file: File | null) => void;
  setActionVideo: (file: File | null) => void;
  handleFiles: (files: File[]) => Promise<void>;
}

/** Action-imitation mode: character still + motion reference upload slots. */
export function ActionUploadSlots({
  loading,
  actionCharacter,
  actionVideo,
  setActionCharacter,
  setActionVideo,
  handleFiles,
}: ActionUploadSlotsProps) {
  const { t } = useTranslation();
  const characterInputRef = useRef<HTMLInputElement | null>(null);
  const videoInputRef = useRef<HTMLInputElement | null>(null);

  return (
    <div className={styles.actionSlots}>
      <button
        type='button'
        className={`${styles.actionSlot} ${
          actionCharacter ? styles.actionSlotFilled : ''
        }`}
        disabled={loading}
        onClick={() => characterInputRef.current?.click()}
        aria-label={t('videoGeneration.create.action.characterAria', {
          defaultValue: '上传角色图（PNG / JPEG / WEBP）',
        })}
      >
        {actionCharacter ? (
          <img
            src={actionCharacter.previewUrl}
            alt=''
            className={styles.actionSlotPreview}
          />
        ) : (
          <People theme='outline' size={22} />
        )}
        <span className={styles.actionSlotMeta}>
          {t('videoGeneration.create.action.character', { defaultValue: '角色图' })}
        </span>
        {actionCharacter ? (
          <span
            role='button'
            tabIndex={0}
            className={styles.actionSlotClear}
            onClick={(event) => {
              event.stopPropagation();
              setActionCharacter(null);
            }}
            onKeyDown={(event) => {
              if (event.key === 'Enter' || event.key === ' ') {
                event.preventDefault();
                event.stopPropagation();
                setActionCharacter(null);
              }
            }}
            aria-label={t('videoGeneration.create.action.removeCharacter', {
              defaultValue: '移除角色图',
            })}
          >
            <CloseSmall size={12} />
          </span>
        ) : null}
      </button>
      <input
        ref={characterInputRef}
        type='file'
        accept={ACTION_CHARACTER_ACCEPT}
        hidden
        disabled={loading}
        onChange={(event) => {
          const file = event.target.files?.[0];
          if (file) void handleFiles([file]);
          event.target.value = '';
        }}
      />
      <button
        type='button'
        className={`${styles.actionSlot} ${
          actionVideo ? styles.actionSlotFilled : ''
        }`}
        disabled={loading}
        onClick={() => videoInputRef.current?.click()}
        aria-label={t('videoGeneration.create.action.videoAria', {
          defaultValue: '上传参考视频（MP4 / WebM / MOV）',
        })}
      >
        {actionVideo ? (
          <video
            src={actionVideo.previewUrl}
            className={styles.actionSlotPreview}
            muted
            playsInline
          />
        ) : (
          <VideoOne theme='outline' size={22} />
        )}
        <span className={styles.actionSlotMeta}>
          {t('videoGeneration.create.action.video', { defaultValue: '参考视频' })}
        </span>
        {actionVideo ? (
          <span
            role='button'
            tabIndex={0}
            className={styles.actionSlotClear}
            onClick={(event) => {
              event.stopPropagation();
              setActionVideo(null);
            }}
            onKeyDown={(event) => {
              if (event.key === 'Enter' || event.key === ' ') {
                event.preventDefault();
                event.stopPropagation();
                setActionVideo(null);
              }
            }}
            aria-label={t('videoGeneration.create.action.removeVideo', {
              defaultValue: '移除参考视频',
            })}
          >
            <CloseSmall size={12} />
          </span>
        ) : null}
      </button>
      <input
        ref={videoInputRef}
        type='file'
        accept={ACTION_VIDEO_ACCEPT}
        hidden
        disabled={loading}
        onChange={(event) => {
          const file = event.target.files?.[0];
          if (file) void handleFiles([file]);
          event.target.value = '';
        }}
      />
    </div>
  );
}
