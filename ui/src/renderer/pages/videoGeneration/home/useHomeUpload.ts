import { useState } from 'react';
import type { Dispatch, SetStateAction } from 'react';
import type { TFunction } from 'i18next';
import { suggestCameoCharacterName } from '../cameoUtils';
import {
  ACTION_CHARACTER_MAX_BYTES,
  ACTION_VIDEO_MAX_BYTES,
  REFERENCE_IMAGE_MAX_BYTES,
  isSupportedImageFile,
  isSupportedTextFile,
  isSupportedVideoFile,
  readUploadedTextFile,
} from './documentUpload';
import type {
  ActionAssetDraft,
  CanvasReferenceDraft,
  VideoCreateDraft,
  VideoHomeMode,
} from './types';
import { usesCanvasReferences } from './types';

const MAX_REFERENCES = 8;

function makeLocalId(prefix: string): string {
  return `${prefix}_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 8)}`;
}

export interface HomeUploadOptions {
  draft: VideoCreateDraft;
  setDraft: Dispatch<SetStateAction<VideoCreateDraft>>;
  mode: VideoHomeMode;
  isAction: boolean;
  loading: boolean;
  t: TFunction;
}

export interface HomeUploadApi {
  /** File-selection / drop pipeline: validate, size-check, then inject assets. */
  handleFiles: (files: File[]) => Promise<void>;
  uploadError: string | null;
  setUploadError: (error: string | null) => void;
  documentName: string | null;
  setDocumentName: (name: string | null) => void;
  setActionCharacter: (file: File | null) => void;
  setActionVideo: (file: File | null) => void;
  removeCanvasReference: (localId: string) => void;
}

/**
 * Upload pipeline for the home composer: file selection / drag-drop validation,
 * action-imitation asset slots, image-reference injection, and text extraction.
 */
export function useHomeUpload({
  draft,
  setDraft,
  mode,
  isAction,
  loading,
  t,
}: HomeUploadOptions): HomeUploadApi {
  const [uploadError, setUploadError] = useState<string | null>(null);
  const [documentName, setDocumentName] = useState<string | null>(null);

  const revokeActionDraft = (asset: ActionAssetDraft | null) => {
    if (asset?.previewUrl) URL.revokeObjectURL(asset.previewUrl);
  };

  const setActionCharacter = (file: File | null) => {
    setDraft((current) => {
      revokeActionDraft(current.actionCharacter);
      if (!file) return { ...current, actionCharacter: null };
      return {
        ...current,
        actionCharacter: { file, previewUrl: URL.createObjectURL(file) },
      };
    });
  };

  const setActionVideo = (file: File | null) => {
    setDraft((current) => {
      revokeActionDraft(current.actionVideo);
      if (!file) return { ...current, actionVideo: null };
      return {
        ...current,
        actionVideo: { file, previewUrl: URL.createObjectURL(file) },
      };
    });
  };

  const addAgentImages = (files: File[]) => {
    const room = Math.max(0, MAX_REFERENCES - draft.cameos.length);
    const added = files.slice(0, room).map((file, index) => ({
      localId: makeLocalId('cameo'),
      characterName: suggestCameoCharacterName(file.name, draft.cameos.length + index),
      description: '',
      file,
      previewUrl: URL.createObjectURL(file),
    }));
    setDraft((current) => ({ ...current, cameos: [...current.cameos, ...added] }));
  };

  const addCanvasImages = (files: File[]) => {
    const room = Math.max(0, MAX_REFERENCES - draft.canvasReferences.length);
    const added: CanvasReferenceDraft[] = files.slice(0, room).map((file) => ({
      localId: makeLocalId('reference'),
      file,
      previewUrl: URL.createObjectURL(file),
    }));
    setDraft((current) => ({
      ...current,
      canvasReferences: [...current.canvasReferences, ...added],
    }));
  };

  const handleFiles = async (files: File[]) => {
    if (loading || files.length === 0) return;
    setUploadError(null);
    if (isAction) {
      const images = files.filter(isSupportedImageFile);
      const videos = files.filter(isSupportedVideoFile);
      const unsupported = files.filter(
        (file) => !isSupportedImageFile(file) && !isSupportedVideoFile(file)
      );
      if (images[0]) {
        if (images[0].size > ACTION_CHARACTER_MAX_BYTES) {
          setUploadError(
            t('videoGeneration.create.action.characterTooLarge', {
              defaultValue: '角色图不能超过 10 MB。',
            })
          );
        } else {
          setActionCharacter(images[0]);
        }
      }
      if (videos[0]) {
        if (videos[0].size > ACTION_VIDEO_MAX_BYTES) {
          setUploadError(
            t('videoGeneration.create.action.videoTooLarge', {
              defaultValue: '参考视频不能超过 80 MB。',
            })
          );
        } else {
          setActionVideo(videos[0]);
        }
      }
      if (unsupported.length > 0) {
        setUploadError(
          t('videoGeneration.create.action.unsupported', {
            defaultValue: '请上传 PNG / JPEG / WEBP 角色图，或 MP4 / WebM / MOV 参考视频。',
          })
        );
      }
      return;
    }
    const images = files.filter(isSupportedImageFile);
    const documents = files.filter(isSupportedTextFile);
    const unsupported = files.filter(
      (file) => !isSupportedImageFile(file) && !isSupportedTextFile(file)
    );
    if (images.length > 0) {
      const oversized = images.filter((file) => file.size > REFERENCE_IMAGE_MAX_BYTES);
      const ok = images.filter((file) => file.size <= REFERENCE_IMAGE_MAX_BYTES);
      if (oversized.length > 0) {
        setUploadError(
          t('videoGeneration.create.upload.imageTooLarge', {
            defaultValue: '参考图不能超过 25 MB（剧照请先压缩后再上传）。',
          })
        );
      }
      if (ok.length > 0) {
        if (usesCanvasReferences(mode)) addCanvasImages(ok);
        else addAgentImages(ok);
      }
    }
    if (documents[0]) {
      try {
        const text = await readUploadedTextFile(documents[0]);
        setDocumentName(documents[0].name);
        setDraft((current) =>
          mode === 'agent'
            ? {
                ...current,
                workflow:
                  current.workflow === 'idea2video' ? 'script2video' : current.workflow,
                sourceText: text,
              }
            : { ...current, creationPrompt: text }
        );
      } catch (error) {
        setUploadError(error instanceof Error ? error.message : String(error));
      }
    }
    if (unsupported.length > 0) {
      setUploadError(
        t('videoGeneration.create.upload.unsupported', {
          defaultValue: '部分文件格式暂不支持，请上传图片、DOCX 或纯文本文档。',
        })
      );
    }
  };

  const removeCanvasReference = (localId: string) => {
    const target = draft.canvasReferences.find((item) => item.localId === localId);
    if (target) URL.revokeObjectURL(target.previewUrl);
    setDraft((current) => ({
      ...current,
      canvasReferences: current.canvasReferences.filter(
        (item) => item.localId !== localId
      ),
    }));
  };

  return {
    handleFiles,
    uploadError,
    setUploadError,
    documentName,
    setDocumentName,
    setActionCharacter,
    setActionVideo,
    removeCanvasReference,
  };
}
