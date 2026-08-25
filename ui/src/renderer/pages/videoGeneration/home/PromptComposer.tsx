import React, { useRef } from 'react';
import { Input } from '@arco-design/web-react';
import { useTranslation } from 'react-i18next';
import { CloseSmall, FileText } from '@icon-park/react';
import { SlantedDocIcon } from './ComposerIcons';
import { displayFileStem, VIDEO_HOME_UPLOAD_ACCEPT } from './documentUpload';
import type { CanvasReferenceDraft, VideoHomeMode } from './types';
import { usesCanvasReferences } from './types';
import styles from './home.module.css';

const TextArea = Input.TextArea;

export interface PromptComposerProps {
  mode: VideoHomeMode;
  loading: boolean;
  documentName: string | null;
  setDocumentName: (name: string | null) => void;
  uploadPreview: string | undefined;
  referenceCount: number;
  canvasReferences: CanvasReferenceDraft[];
  removeCanvasReference: (localId: string) => void;
  selectedVerticalSkills: ReadonlyArray<{ id: string; label: string }>;
  removeVerticalSkill: (skillId: string) => void;
  activeText: string;
  setActiveText: (value: string) => void;
  placeholder: string;
  handlePromptKeyDown: (event: React.KeyboardEvent<HTMLTextAreaElement>) => void;
  handleFiles: (files: File[]) => Promise<void>;
}

/** Agent / creation prompt area: upload slot, inline attachments, skill chips, editor. */
export function PromptComposer({
  mode,
  loading,
  documentName,
  setDocumentName,
  uploadPreview,
  referenceCount,
  canvasReferences,
  removeCanvasReference,
  selectedVerticalSkills,
  removeVerticalSkill,
  activeText,
  setActiveText,
  placeholder,
  handlePromptKeyDown,
  handleFiles,
}: PromptComposerProps) {
  const { t } = useTranslation();
  const fileInputRef = useRef<HTMLInputElement | null>(null);

  return (
    <div className={styles.composerMain}>
      <button
        type='button'
        className={`${styles.uploadSlot} ${
          uploadPreview ? styles.uploadSlotFilled : ''
        } ${documentName && !uploadPreview ? styles.uploadSlotDocument : ''}`}
        disabled={loading}
        onClick={() => fileInputRef.current?.click()}
        title={t('videoGeneration.create.upload.aria', {
          defaultValue:
            '上传角色参考图、剧本或资料文档（PNG / JPEG / WEBP / DOCX / TXT / Markdown 等）',
        })}
        aria-label={t('videoGeneration.create.upload.aria', {
          defaultValue:
            '上传角色参考图、剧本或资料文档（PNG / JPEG / WEBP / DOCX / TXT / Markdown 等）',
        })}
      >
        {uploadPreview ? (
          <img src={uploadPreview} alt='' className={styles.uploadPreview} />
        ) : (
          <span
            className={`${styles.uploadGlyph} ${
              documentName ? styles.uploadGlyphActive : ''
            }`}
            aria-hidden='true'
          >
            <SlantedDocIcon size={24} className={styles.uploadDocIcon} />
          </span>
        )}
        {referenceCount > 1 ? (
          <em className={styles.uploadCount}>+{referenceCount - 1}</em>
        ) : documentName && uploadPreview ? (
          <em className={styles.uploadDocBadge} aria-hidden='true'>
            <FileText size={11} />
          </em>
        ) : null}
      </button>
      <input
        ref={fileInputRef}
        type='file'
        accept={VIDEO_HOME_UPLOAD_ACCEPT}
        multiple
        hidden
        disabled={loading}
        onChange={(event) => {
          void handleFiles(Array.from(event.target.files ?? []));
          event.target.value = '';
        }}
      />

      <div className={styles.promptArea}>
        <div className={styles.promptInner}>
          {(documentName ||
            (usesCanvasReferences(mode) && canvasReferences.length > 0)) && (
            <div className={styles.inlineAttachments}>
              {documentName ? (
                <span className={styles.documentChip}>
                  <FileText size={13} />
                  {displayFileStem(documentName)}
                  <button
                    type='button'
                    aria-label={t('videoGeneration.create.upload.removeDocument', {
                      defaultValue: '移除文档',
                    })}
                    onClick={() => {
                      setDocumentName(null);
                      setActiveText('');
                    }}
                  >
                    <CloseSmall size={12} />
                  </button>
                </span>
              ) : null}
              {usesCanvasReferences(mode)
                ? canvasReferences.slice(0, 4).map((reference) => (
                    <span key={reference.localId} className={styles.referenceThumb}>
                      <img src={reference.previewUrl} alt={reference.file.name} />
                      <button
                        type='button'
                        aria-label={t('videoGeneration.create.upload.removeReference', {
                          name: reference.file.name,
                          defaultValue: '移除 {{name}}',
                        })}
                        onClick={() => removeCanvasReference(reference.localId)}
                      >
                        <CloseSmall size={12} />
                      </button>
                    </span>
                  ))
                : null}
            </div>
          )}
          <div className={styles.promptEditor}>
            {mode === 'agent' && selectedVerticalSkills.length > 0 ? (
              <div className={styles.skillChips}>
                {selectedVerticalSkills.map((skill, index) => (
                  <React.Fragment key={skill.id}>
                    {index > 0 ? (
                      <span className={styles.skillDiamond} aria-hidden='true' />
                    ) : null}
                    <button
                      type='button'
                      className={styles.skillTag}
                      disabled={loading}
                      title={skill.label}
                      aria-label={t('videoGeneration.skills.removeSelected', {
                        name: skill.label,
                        defaultValue: '移除 Skill {{name}}',
                      })}
                      onClick={() => removeVerticalSkill(skill.id)}
                      onKeyDown={(event) => {
                        if (event.key === 'Backspace' || event.key === 'Delete') {
                          event.preventDefault();
                          event.stopPropagation();
                          removeVerticalSkill(skill.id);
                        }
                      }}
                    >
                      <strong>{skill.label}</strong>
                      <CloseSmall size={11} />
                    </button>
                  </React.Fragment>
                ))}
              </div>
            ) : null}
            <TextArea
              value={activeText}
              onChange={setActiveText}
              placeholder={
                mode === 'agent' && selectedVerticalSkills.length > 0
                  ? ''
                  : placeholder
              }
              disabled={loading}
              className={styles.promptInput}
              onKeyDown={handlePromptKeyDown}
            />
          </div>
        </div>
      </div>
    </div>
  );
}
