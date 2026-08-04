/**
 * Editable artifact preview for Technical artifacts & run files.
 *
 * - Text / JSON: inline edit + save
 * - Images: replace from disk, or regenerate via inline prompt edit
 */
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, Input, Modal, Spin } from '@arco-design/web-react';
import { Edit, Refresh, Upload } from '@icon-park/react';
import {
  getArtifactImagePrompt,
  replaceArtifactFile,
  updateArtifactImagePrompt,
  writeArtifactText,
} from '../api';
import type { ArtifactContent } from '../types';

const TextArea = Input.TextArea;

function isVideoPath(path: string): boolean {
  return /\.(mp4|webm|mov|avi|mkv)$/i.test(path);
}

function isImagePath(path: string): boolean {
  return /\.(png|jpe?g|gif|webp|bmp)$/i.test(path);
}

function isEditableTextPath(path: string): boolean {
  return /\.(txt|md|json|csv|prompt)$/i.test(path) || path.toLowerCase().endsWith('.json');
}

export interface ArtifactPreviewPanelProps {
  sessionId: string;
  selectedPath: string | null;
  preview: ArtifactContent | null;
  previewLoading: boolean;
  disabled?: boolean;
  onChanged: () => void;
  /** Start render after prompt update so the image regenerates immediately. */
  onRequestRegenerate?: () => void;
}

const ArtifactPreviewPanel: React.FC<ArtifactPreviewPanelProps> = ({
  sessionId,
  selectedPath,
  preview,
  previewLoading,
  disabled,
  onChanged,
  onRequestRegenerate,
}) => {
  const { t } = useTranslation();
  const fileInputRef = useRef<HTMLInputElement>(null);

  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState('');
  const [saving, setSaving] = useState(false);

  const [regenMode, setRegenMode] = useState(false);
  const [promptDraft, setPromptDraft] = useState('');
  const [promptLoading, setPromptLoading] = useState(false);
  const [promptSaving, setPromptSaving] = useState(false);
  const [replacing, setReplacing] = useState(false);

  const path = selectedPath ?? '';
  const canEditText =
    Boolean(path) &&
    isEditableTextPath(path) &&
    (preview?.kind === 'text' || preview?.kind === 'json') &&
    preview.text != null;
  const canEditImage = Boolean(path) && isImagePath(path) && preview?.kind === 'url';

  useEffect(() => {
    setEditing(false);
    setDraft(preview?.text ?? '');
    setRegenMode(false);
    setPromptDraft('');
  }, [path, preview?.text]);

  const startEdit = useCallback(() => {
    setDraft(preview?.text ?? '');
    setEditing(true);
  }, [preview?.text]);

  const cancelEdit = useCallback(() => {
    setDraft(preview?.text ?? '');
    setEditing(false);
  }, [preview?.text]);

  const handleSaveText = useCallback(async () => {
    if (!path || disabled) return;
    setSaving(true);
    try {
      await writeArtifactText(sessionId, path, draft);
      setEditing(false);
      onChanged();
    } catch (e) {
      Modal.error({
        title: t('videoGeneration.artifacts.saveFailed', { defaultValue: '保存失败' }),
        content: e instanceof Error ? e.message : String(e),
      });
    } finally {
      setSaving(false);
    }
  }, [disabled, draft, onChanged, path, sessionId, t]);

  const handleReplaceClick = useCallback(() => {
    fileInputRef.current?.click();
  }, []);

  const handleFilePicked = useCallback(
    async (event: React.ChangeEvent<HTMLInputElement>) => {
      const file = event.target.files?.[0];
      event.target.value = '';
      if (!file || !path || disabled) return;
      setReplacing(true);
      try {
        await replaceArtifactFile(sessionId, path, file);
        onChanged();
      } catch (e) {
        Modal.error({
          title: t('videoGeneration.artifacts.replaceFailed', { defaultValue: '替换失败' }),
          content: e instanceof Error ? e.message : String(e),
        });
      } finally {
        setReplacing(false);
      }
    },
    [disabled, onChanged, path, sessionId, t]
  );

  const startRegenerate = useCallback(async () => {
    if (!path || disabled) return;
    setRegenMode(true);
    setPromptLoading(true);
    try {
      const info = await getArtifactImagePrompt(sessionId, path);
      setPromptDraft(info.prompt || '');
      if (!info.prompt?.trim()) {
        Modal.warning({
          title: t('videoGeneration.artifacts.promptMissingTitle', {
            defaultValue: '未找到原始生图提示词',
          }),
          content: t('videoGeneration.artifacts.promptMissingHint', {
            defaultValue:
              '该图片没有保存完整生成提示词（可能是旧任务产物）。请手动填写后重新生成；新生成的图片会自动保留提示词。',
          }),
        });
      }
    } catch (e) {
      setPromptDraft('');
      Modal.warning({
        title: t('videoGeneration.artifacts.promptLoadFailed', {
          defaultValue: '无法加载生图提示词',
        }),
        content: e instanceof Error ? e.message : String(e),
      });
    } finally {
      setPromptLoading(false);
    }
  }, [disabled, path, sessionId, t]);

  const cancelRegenerate = useCallback(() => {
    setRegenMode(false);
    setPromptDraft('');
  }, []);

  const handleRegenerate = useCallback(async () => {
    if (!path || disabled) return;
    const trimmed = promptDraft.trim();
    if (!trimmed) return;
    setPromptSaving(true);
    try {
      await updateArtifactImagePrompt(sessionId, path, trimmed);
      setRegenMode(false);
      onChanged();
      onRequestRegenerate?.();
    } catch (e) {
      Modal.error({
        title: t('videoGeneration.artifacts.promptSaveFailed', {
          defaultValue: '提示词保存失败',
        }),
        content: e instanceof Error ? e.message : String(e),
      });
    } finally {
      setPromptSaving(false);
    }
  }, [disabled, onChanged, onRequestRegenerate, path, promptDraft, sessionId, t]);

  const toolbar = useMemo(() => {
    if (!path || disabled) return null;
    if (canEditText && !editing) {
      return (
        <Button size='mini' type='outline' onClick={startEdit}>
          <span className='inline-flex items-center gap-4px'>
            <Edit theme='outline' size={12} />
            {t('videoGeneration.artifacts.edit', { defaultValue: '编辑' })}
          </span>
        </Button>
      );
    }
    if (canEditText && editing) {
      return (
        <div className='flex items-center gap-6px'>
          <Button size='mini' onClick={cancelEdit} disabled={saving}>
            {t('common.cancel', { defaultValue: '取消' })}
          </Button>
          <Button size='mini' type='primary' loading={saving} onClick={() => void handleSaveText()}>
            {t('videoGeneration.artifacts.save', { defaultValue: '保存' })}
          </Button>
        </div>
      );
    }
    if (canEditImage && regenMode) {
      return (
        <div className='flex items-center gap-6px'>
          <Button size='mini' onClick={cancelRegenerate} disabled={promptSaving}>
            {t('common.cancel', { defaultValue: '取消' })}
          </Button>
          <Button
            size='mini'
            type='primary'
            loading={promptSaving}
            disabled={!promptDraft.trim() || !onRequestRegenerate}
            onClick={() => void handleRegenerate()}
          >
            {t('videoGeneration.artifacts.regenerate', { defaultValue: '重新生成' })}
          </Button>
        </div>
      );
    }
    if (canEditImage) {
      return (
        <div className='flex flex-wrap items-center gap-6px'>
          <Button size='mini' type='outline' loading={replacing} onClick={handleReplaceClick}>
            <span className='inline-flex items-center gap-4px'>
              <Upload theme='outline' size={12} />
              {t('videoGeneration.artifacts.replaceImage', { defaultValue: '本地替换' })}
            </span>
          </Button>
          <Button size='mini' type='outline' onClick={() => void startRegenerate()}>
            <span className='inline-flex items-center gap-4px'>
              <Refresh theme='outline' size={12} />
              {t('videoGeneration.artifacts.regenerate', { defaultValue: '重新生成' })}
            </span>
          </Button>
        </div>
      );
    }
    return null;
  }, [
    canEditImage,
    canEditText,
    cancelEdit,
    cancelRegenerate,
    disabled,
    editing,
    handleRegenerate,
    handleReplaceClick,
    handleSaveText,
    onRequestRegenerate,
    path,
    promptDraft,
    promptSaving,
    regenMode,
    replacing,
    saving,
    startEdit,
    startRegenerate,
    t,
  ]);

  return (
    <div className='flex min-h-200px max-h-420px flex-col overflow-hidden rd-8px border border-solid border-[var(--color-border-2)] bg-[var(--color-fill-1)]'>
      <div className='flex items-center justify-between gap-8px border-b border-l-0 border-r-0 border-t-0 border-solid border-[var(--color-border-2)] px-10px py-8px'>
        <div className='min-w-0 truncate text-11px text-[var(--color-text-3)]'>
          {selectedPath ??
            t('videoGeneration.workspace.artifacts.selectHint', {
              defaultValue: '选择左侧文件以预览',
            })}
        </div>
        {toolbar}
      </div>
      <div className='flex-1 overflow-auto p-12px'>
        {previewLoading ? (
          <div className='flex justify-center py-40px'>
            <Spin />
          </div>
        ) : editing && canEditText ? (
          <TextArea
            value={draft}
            onChange={setDraft}
            autoSize={{ minRows: 12, maxRows: 22 }}
            className='!font-mono !text-12px !leading-18px'
            disabled={saving}
          />
        ) : regenMode && canEditImage ? (
          promptLoading ? (
            <div className='flex justify-center py-40px'>
              <Spin />
            </div>
          ) : (
            <div className='flex h-full min-h-180px flex-col gap-8px'>
              <div className='text-11px font-600 text-[var(--color-text-2)]'>
                {t('videoGeneration.artifacts.promptTitle', {
                  defaultValue: '生图提示词',
                })}
              </div>
              <p className='m-0 text-11px text-[var(--color-text-3)]'>
                {t('videoGeneration.artifacts.regenHint', {
                  defaultValue: '编辑提示词后点击「重新生成」，将清除当前图并启动渲染补全新图。',
                })}
              </p>
              <TextArea
                value={promptDraft}
                onChange={setPromptDraft}
                autoSize={{ minRows: 8, maxRows: 16 }}
                disabled={promptSaving}
                className='flex-1 !text-13px !leading-20px'
                placeholder={t('videoGeneration.artifacts.promptPlaceholder', {
                  defaultValue: '描述你想要的画面…',
                })}
              />
            </div>
          )
        ) : preview?.kind === 'url' && preview.url && selectedPath ? (
          isVideoPath(selectedPath) || preview.mime?.startsWith('video/') ? (
            <video src={preview.url} controls className='max-h-360px max-w-full rd-8px' />
          ) : (
            <img
              src={preview.url}
              alt={selectedPath}
              className='max-h-360px max-w-full rd-8px object-contain'
            />
          )
        ) : preview?.text != null ? (
          <pre className='m-0 whitespace-pre-wrap break-words font-mono text-12px leading-18px text-[var(--color-text-1)]'>
            {preview.text}
          </pre>
        ) : (
          <div className='text-12px text-[var(--color-text-3)]'>
            {t('videoGeneration.workspace.artifacts.selectHint', {
              defaultValue: '选择左侧文件以预览',
            })}
          </div>
        )}
      </div>

      <input
        ref={fileInputRef}
        type='file'
        accept='image/png,image/jpeg,image/webp,image/gif,image/bmp,.png,.jpg,.jpeg,.webp,.gif,.bmp'
        className='hidden'
        onChange={(e) => void handleFilePicked(e)}
      />
    </div>
  );
};

export default ArtifactPreviewPanel;
