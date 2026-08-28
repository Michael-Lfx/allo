import React, { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { Input } from '@arco-design/web-react';
import { useTranslation } from 'react-i18next';
import { CloseSmall, FileText } from '@icon-park/react';
import { AttachCloseIcon, AttachPlusIcon } from './ComposerIcons';
import { displayFileStem } from './documentUpload';
import type { CameoDraftItem } from '../types';
import type { CanvasReferenceDraft, VideoHomeMode } from './types';
import { usesCanvasReferences } from './types';
import styles from './home.module.css';

const TextArea = Input.TextArea;

const FAN_TILTS = [-13, 9, -7, 12, -10, 6, -5, 11];

export interface PromptComposerProps {
  mode: VideoHomeMode;
  loading: boolean;
  documentName: string | null;
  setDocumentName: (name: string | null) => void;
  canvasReferences: CanvasReferenceDraft[];
  removeCanvasReference: (localId: string) => void;
  cameos: CameoDraftItem[];
  removeCameo: (localId: string) => void;
  selectedVerticalSkills: ReadonlyArray<{ id: string; label: string }>;
  removeVerticalSkill: (skillId: string) => void;
  activeText: string;
  setActiveText: (value: string) => void;
  placeholder: string;
  handlePromptKeyDown: (event: React.KeyboardEvent<HTMLTextAreaElement>) => void;
  onRequestUpload: () => void;
}

/** Jimeng-style attach: piled cards, overlay-spread on hover, plus on the front card. */
export function PromptComposer({
  mode,
  loading,
  documentName,
  setDocumentName,
  canvasReferences,
  removeCanvasReference,
  cameos,
  removeCameo,
  selectedVerticalSkills,
  removeVerticalSkill,
  activeText,
  setActiveText,
  placeholder,
  handlePromptKeyDown,
  onRequestUpload,
}: PromptComposerProps) {
  const { t } = useTranslation();
  const [fanOpen, setFanOpen] = useState(false);
  const enteredIdsRef = useRef(new Set<string>());
  const [enteringIds, setEnteringIds] = useState<Set<string>>(() => new Set());

  const imageItems = usesCanvasReferences(mode)
    ? canvasReferences.map((reference) => ({
        id: reference.localId,
        previewUrl: reference.previewUrl,
        name: reference.file.name,
        onRemove: () => removeCanvasReference(reference.localId),
      }))
    : cameos.flatMap((cameo) =>
        cameo.previewUrl
          ? [
              {
                id: cameo.localId,
                previewUrl: cameo.previewUrl,
                name: cameo.characterName || cameo.file?.name || '',
                onRemove: () => removeCameo(cameo.localId),
              },
            ]
          : []
      );

  const stackedCount = imageItems.length + (documentName ? 1 : 0);
  const itemIdsKey =
    imageItems.map((item) => item.id).join('|') + (documentName ? '|__doc__' : '');

  useLayoutEffect(() => {
    const ids = itemIdsKey.length > 0 ? itemIdsKey.split('|') : [];
    const fresh = ids.filter((id) => !enteredIdsRef.current.has(id));
    if (fresh.length === 0) return undefined;
    for (const id of fresh) enteredIdsRef.current.add(id);
    setEnteringIds(new Set(fresh));
    const timer = window.setTimeout(() => setEnteringIds(new Set()), 480);
    return () => window.clearTimeout(timer);
  }, [itemIdsKey]);

  useEffect(() => {
    if (stackedCount === 0) setFanOpen(false);
  }, [stackedCount]);

  const removeWithoutReload = (event: React.MouseEvent, remove: () => void) => {
    event.preventDefault();
    event.stopPropagation();
    remove();
  };

  return (
    <div className={styles.composerMain}>
      <div className={styles.promptArea}>
        <div className={styles.promptInner}>
          <div className={styles.attachStage}>
            {stackedCount > 0 ? (
              <div
                className={`${styles.attachFan} ${fanOpen ? styles.attachFanOpen : ''}`}
                style={{ ['--count' as string]: stackedCount }}
                onMouseEnter={() => setFanOpen(true)}
                onMouseLeave={() => setFanOpen(false)}
              >
                {imageItems.map((item, index) => (
                  <span
                    key={item.id}
                    className={`${styles.attachPhoto} ${
                      enteringIds.has(item.id) ? styles.attachPhotoEnter : ''
                    }`}
                    style={{
                      ['--i' as string]: index,
                      ['--tilt' as string]: `${FAN_TILTS[index % FAN_TILTS.length]}deg`,
                    }}
                  >
                    <span className={styles.attachPhotoFace}>
                      <img src={item.previewUrl} alt='' />
                    </span>
                    <button
                      type='button'
                      className={styles.attachRemove}
                      disabled={loading}
                      aria-label={t('videoGeneration.create.upload.removeReference', {
                        name: item.name,
                        defaultValue: '移除 {{name}}',
                      })}
                      onMouseDown={(event) => event.preventDefault()}
                      onClick={(event) => removeWithoutReload(event, item.onRemove)}
                    >
                      <AttachCloseIcon />
                    </button>
                  </span>
                ))}
                {documentName ? (
                  <span
                    className={`${styles.attachPhoto} ${
                      enteringIds.has('__doc__') ? styles.attachPhotoEnter : ''
                    }`}
                    style={{
                      ['--i' as string]: imageItems.length,
                      ['--tilt' as string]: `${FAN_TILTS[imageItems.length % FAN_TILTS.length]}deg`,
                    }}
                  >
                    <span className={`${styles.attachPhotoFace} ${styles.attachDocPhoto}`}>
                      <FileText size={16} />
                      <em>{displayFileStem(documentName)}</em>
                    </span>
                    <button
                      type='button'
                      className={styles.attachRemove}
                      disabled={loading}
                      aria-label={t('videoGeneration.create.upload.removeDocument', {
                        defaultValue: '移除文档',
                      })}
                      onMouseDown={(event) => event.preventDefault()}
                      onClick={(event) =>
                        removeWithoutReload(event, () => {
                          setDocumentName(null);
                          setActiveText('');
                        })
                      }
                    >
                      <AttachCloseIcon />
                    </button>
                  </span>
                ) : null}
                <button
                  type='button'
                  className={styles.attachPlusBadge}
                  disabled={loading}
                  onClick={onRequestUpload}
                  aria-label={t('videoGeneration.create.upload.addReference', {
                    defaultValue: '上传文件',
                  })}
                >
                  <AttachPlusIcon size={11} />
                </button>
              </div>
            ) : (
              <button
                type='button'
                className={`${styles.attachPlusCard} ${styles.attachPlusCardSolo}`}
                disabled={loading}
                onClick={onRequestUpload}
                aria-label={t('videoGeneration.create.upload.addReference', {
                  defaultValue: '上传文件',
                })}
              >
                <AttachPlusIcon size={15} />
              </button>
            )}
          </div>
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
