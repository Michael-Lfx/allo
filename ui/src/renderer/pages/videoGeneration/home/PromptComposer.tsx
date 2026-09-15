import React, { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { Input } from '@arco-design/web-react';
import { useTranslation } from 'react-i18next';
import { FileText } from '@icon-park/react';
import { AttachCloseIcon, AttachPlusIcon } from './ComposerIcons';
import type { TFunction } from 'i18next';
import { displayFileStem } from './documentUpload';
import type { CameoDraftItem } from '../types';
import type { CanvasReferenceDraft, VideoHomeMode } from './types';
import { usesCanvasReferences } from './types';
import type { CreationSubjectKind } from '@renderer/pages/videoCanvas/lib/creation-ir';
import {
  detectHomeImageMentionQuery,
  filterHomeImageMentionCandidates,
  homeImageLabel,
  insertHomeImageMention,
  splitHomeImageMentionParts,
} from './imageMentions';
import {
  getTextareaCaretViewportRect,
  setHomeMentionTextareaFill,
  syncHomeMentionHighlightOverlay,
  type TextareaCaretRect,
} from './mentionCaret';
import { ImageMentionMenu, type HomeImageMentionCandidate } from './ImageMentionMenu';
import styles from './home.module.css';

const TextArea = Input.TextArea;

const FAN_TILTS = [-13, 9, -7, 12, -10, 6, -5, 11];
const SUBJECT_KINDS: CreationSubjectKind[] = ['character', 'scene', 'prop'];

type AttachImageItem = {
  id: string;
  previewUrl: string;
  name: string;
  onRemove: () => void;
  subjectKind?: CreationSubjectKind;
  onPatch?: (patch: Partial<Pick<CanvasReferenceDraft, 'subjectKind' | 'subjectName'>>) => void;
};

export interface PromptComposerProps {
  mode: VideoHomeMode;
  loading: boolean;
  documentName: string | null;
  setDocumentName: (name: string | null) => void;
  canvasReferences: CanvasReferenceDraft[];
  removeCanvasReference: (localId: string) => void;
  updateCanvasReference?: (localId: string, patch: Partial<Pick<CanvasReferenceDraft, 'subjectKind' | 'subjectName'>>) => void;
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
  updateCanvasReference,
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
  const promptEditorRef = useRef<HTMLDivElement>(null);
  const promptInputShellRef = useRef<HTMLDivElement>(null);
  const highlightRef = useRef<HTMLDivElement>(null);
  const cursorRef = useRef(0);
  const [mentionQuery, setMentionQuery] = useState<{ start: number; query: string } | null>(null);
  const [mentionIndex, setMentionIndex] = useState(0);
  const [mentionCaret, setMentionCaret] = useState<TextareaCaretRect | null>(null);

  const imageItems: AttachImageItem[] = mode === 'briefing'
    ? []
    : usesCanvasReferences(mode)
    ? canvasReferences.map((reference) => ({
        id: reference.localId,
        previewUrl: reference.previewUrl,
        name: reference.subjectName || reference.file.name,
        subjectKind: reference.subjectKind,
        onRemove: () => removeCanvasReference(reference.localId),
        onPatch: updateCanvasReference
          ? (patch: Partial<Pick<CanvasReferenceDraft, 'subjectKind' | 'subjectName'>>) =>
              updateCanvasReference(reference.localId, patch)
          : undefined,
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

  const mentionCandidates = useMemo<HomeImageMentionCandidate[]>(
    () =>
      imageItems.map((item, index) => ({
        index,
        label: homeImageLabel(index),
        name: item.name,
        previewUrl: item.previewUrl,
      })),
    [imageItems],
  );

  const visibleMentions = useMemo(
    () =>
      mentionQuery && mentionCandidates.length > 0
        ? filterHomeImageMentionCandidates(mentionCandidates, mentionQuery.query)
        : [],
    [mentionCandidates, mentionQuery],
  );

  const mentionParts = useMemo(() => splitHomeImageMentionParts(activeText), [activeText]);
  const highlightMentions = mentionParts.some((part) => part.type === 'mention');

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

  useEffect(() => {
    if (mentionCandidates.length === 0) setMentionQuery(null);
  }, [mentionCandidates.length]);

  useEffect(() => {
    setMentionIndex(0);
  }, [mentionQuery?.start, mentionQuery?.query, visibleMentions.length]);

  useEffect(() => {
    if (!mentionQuery) return undefined;
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target;
      if (!(target instanceof Node)) return;
      if (promptEditorRef.current?.contains(target)) return;
      if (target instanceof Element && target.closest(`.${styles.mentionMenu}`)) return;
      setMentionQuery(null);
    };
    window.addEventListener('pointerdown', onPointerDown);
    return () => window.removeEventListener('pointerdown', onPointerDown);
  }, [mentionQuery]);

  const textareaEl = () => promptEditorRef.current?.querySelector('textarea') ?? null;

  useLayoutEffect(() => {
    if (!mentionQuery) {
      setMentionCaret(null);
      return undefined;
    }
    const measure = () => {
      const textarea = textareaEl();
      if (!textarea) {
        setMentionCaret(null);
        return;
      }
      setMentionCaret(getTextareaCaretViewportRect(textarea, mentionQuery.start));
    };
    measure();
    const textarea = textareaEl();
    window.addEventListener('resize', measure);
    textarea?.addEventListener('scroll', measure);
    return () => {
      window.removeEventListener('resize', measure);
      textarea?.removeEventListener('scroll', measure);
    };
  }, [mentionQuery, activeText]);

  useLayoutEffect(() => {
    const textarea = textareaEl();
    const highlight = highlightRef.current;
    const shell = promptInputShellRef.current;
    if (!textarea) return undefined;
    if (!highlightMentions || !highlight || !shell) {
      setHomeMentionTextareaFill(textarea, false);
      return undefined;
    }
    const sync = () => {
      syncHomeMentionHighlightOverlay(textarea, highlight, shell);
      setHomeMentionTextareaFill(textarea, true);
    };
    sync();
    textarea.addEventListener('scroll', sync);
    window.addEventListener('resize', sync);
    return () => {
      textarea.removeEventListener('scroll', sync);
      window.removeEventListener('resize', sync);
    };
  }, [highlightMentions, activeText]);

  const rememberCursor = (target: EventTarget | null) => {
    if (!(target instanceof HTMLTextAreaElement)) return;
    cursorRef.current = target.selectionStart ?? cursorRef.current;
  };

  const syncMention = (value: string, cursor: number) => {
    cursorRef.current = cursor;
    if (mentionCandidates.length === 0) {
      setMentionQuery(null);
      return;
    }
    setMentionQuery(detectHomeImageMentionQuery(value, cursor));
  };

  const applyText = (next: string, cursor: number) => {
    setActiveText(next);
    cursorRef.current = cursor;
    requestAnimationFrame(() => {
      const textarea = textareaEl();
      if (!textarea) return;
      textarea.focus();
      textarea.setSelectionRange(cursor, cursor);
      syncMention(next, cursor);
    });
  };

  const insertMention = (index: number) => {
    const cursor = textareaEl()?.selectionStart ?? cursorRef.current;
    const next = insertHomeImageMention(activeText, cursor, index);
    setMentionQuery(null);
    applyText(next.text, next.cursor);
  };

  const removeWithoutReload = (event: React.MouseEvent, remove: () => void) => {
    event.preventDefault();
    event.stopPropagation();
    remove();
  };

  const onPromptKeyDown = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    rememberCursor(event.currentTarget);
    if (mentionQuery && mentionCandidates.length > 0) {
      if (event.key === 'ArrowDown') {
        event.preventDefault();
        setMentionIndex((current) =>
          visibleMentions.length === 0 ? 0 : (current + 1) % visibleMentions.length,
        );
        return;
      }
      if (event.key === 'ArrowUp') {
        event.preventDefault();
        setMentionIndex((current) =>
          visibleMentions.length === 0
            ? 0
            : (current - 1 + visibleMentions.length) % visibleMentions.length,
        );
        return;
      }
      if (event.key === 'Enter' && !event.shiftKey && !event.metaKey && !event.ctrlKey) {
        const picked = visibleMentions[mentionIndex];
        if (picked) {
          event.preventDefault();
          insertMention(picked.index);
          return;
        }
      }
      if (event.key === 'Tab' && visibleMentions[mentionIndex]) {
        event.preventDefault();
        insertMention(visibleMentions[mentionIndex].index);
        return;
      }
      if (event.key === 'Escape') {
        event.preventDefault();
        setMentionQuery(null);
        return;
      }
    }
    handlePromptKeyDown(event);
  };

  return (
    <div className={styles.composerMain}>
      <div className={styles.promptArea}>
        <div className={styles.promptInner}>
          {mode === 'briefing' ? null : (
          <div className={`${styles.attachStage} ${mode === 'creation' && stackedCount > 0 ? styles.attachStageLabeled : ''}`}>
            {stackedCount > 0 ? (
              <div
                className={`${styles.attachFan} ${fanOpen || (mode === 'creation' && stackedCount > 0) ? styles.attachFanOpen : ''} ${
                  mode === 'creation' && stackedCount > 0 ? styles.attachFanLabeled : ''
                }`}
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
                    <button
                      type='button'
                      className={styles.attachPhotoFaceButton}
                      disabled={loading}
                      aria-label={t('videoGeneration.create.composer.insertMention', {
                        label: homeImageLabel(index),
                        defaultValue: '插入 {{label}}',
                      })}
                      onMouseDown={(event) => event.preventDefault()}
                      onClick={() => insertMention(index)}
                    >
                      <span className={styles.attachPhotoFace}>
                        <img src={item.previewUrl} alt='' />
                        {imageItems.length > 1 ? (
                          <span className={styles.attachIndexBadge}>{index + 1}</span>
                        ) : null}
                      </span>
                    </button>
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
                    {mode === 'creation' && item.onPatch ? (
                      <span className={styles.attachSubjectMeta}>
                        <button
                          type='button'
                          className={styles.attachKindChip}
                          disabled={loading}
                          aria-label={t('videoGeneration.create.upload.subjectKindAria', {
                            defaultValue: '切换主体类型',
                          })}
                          onMouseDown={(event) => event.preventDefault()}
                          onClick={(event) => {
                            event.preventDefault();
                            event.stopPropagation();
                            const current = item.subjectKind && SUBJECT_KINDS.includes(item.subjectKind)
                              ? item.subjectKind
                              : 'character';
                            const next = SUBJECT_KINDS[(SUBJECT_KINDS.indexOf(current) + 1) % SUBJECT_KINDS.length];
                            item.onPatch?.({ subjectKind: next });
                          }}
                        >
                          {subjectKindLabel(t, item.subjectKind)}
                        </button>
                        <input
                          className={styles.attachNameInput}
                          disabled={loading}
                          value={item.name}
                          maxLength={24}
                          placeholder={t('videoGeneration.create.upload.subjectNamePlaceholder', {
                            defaultValue: '名称',
                          })}
                          aria-label={t('videoGeneration.create.upload.subjectNamePlaceholder', {
                            defaultValue: '名称',
                          })}
                          onMouseDown={(event) => event.stopPropagation()}
                          onClick={(event) => event.stopPropagation()}
                          onChange={(event) => item.onPatch?.({ subjectName: event.target.value })}
                        />
                      </span>
                    ) : null}
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
                className={styles.attachPlusCard}
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
          )}
          <div className={styles.promptEditor} ref={promptEditorRef}>
            {mode === 'agent' && selectedVerticalSkills.length > 0 ? (
              <div
                className={styles.skillChips}
                role='list'
                aria-label={t('videoGeneration.skills.selected', { defaultValue: '已选 Skill' })}
              >
                {selectedVerticalSkills.map((skill) => (
                    <button
                      key={skill.id}
                      type='button'
                      role='listitem'
                      className={styles.skillTag}
                      disabled={loading}
                      title={skill.label}
                      aria-pressed='true'
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
                      <span className={styles.skillTagDismiss} aria-hidden='true'>
                        <SkillTagClose />
                      </span>
                    </button>
                ))}
              </div>
            ) : null}
            <div
              ref={promptInputShellRef}
              className={`${styles.promptInputShell} ${highlightMentions ? styles.promptInputShellHighlight : ''}`}
            >
              {highlightMentions ? (
                <div ref={highlightRef} className={styles.promptHighlight} aria-hidden>
                  {mentionParts.map((part, index) =>
                    part.type === 'mention' ? (
                      <span key={index} className={styles.promptMention}>
                        {part.value}
                      </span>
                    ) : (
                      <span key={index}>{part.value}</span>
                    ),
                  )}
                  {activeText.endsWith('\n') ? '\n' : null}
                </div>
              ) : null}
              <TextArea
              value={activeText}
              onChange={(value, event) => {
                setActiveText(value);
                const target = event?.target as HTMLTextAreaElement | undefined;
                syncMention(value, target?.selectionStart ?? value.length);
              }}
              placeholder={
                mode === 'agent' && selectedVerticalSkills.length > 0
                  ? ''
                  : placeholder
              }
              disabled={loading}
              className={styles.promptInput}
              onKeyDown={onPromptKeyDown}
              onClick={(event) => {
                rememberCursor(event.target);
                syncMention(activeText, cursorRef.current);
              }}
              onKeyUp={(event) => {
                rememberCursor(event.currentTarget);
                syncMention(activeText, event.currentTarget.selectionStart ?? cursorRef.current);
              }}
            />
            </div>
            {mentionQuery && mentionCandidates.length > 0 && mentionCaret ? (
              <ImageMentionMenu
                caret={mentionCaret}
                candidates={visibleMentions}
                activeIndex={mentionIndex}
                onSelect={insertMention}
              />
            ) : null}
          </div>
        </div>
      </div>
    </div>
  );
}

function SkillTagClose() {
  return (
    <svg width='8' height='8' viewBox='0 0 8 8' fill='none'>
      <path
        d='M1.6 1.6l4.8 4.8M6.4 1.6l-4.8 4.8'
        stroke='currentColor'
        strokeWidth='1.2'
        strokeLinecap='round'
      />
    </svg>
  );
}

function subjectKindLabel(t: TFunction, kind: CreationSubjectKind | undefined) {
  if (kind === 'scene') {
    return t('videoGeneration.create.upload.subjectKindScene', { defaultValue: '场景' });
  }
  if (kind === 'prop') {
    return t('videoGeneration.create.upload.subjectKindProp', { defaultValue: '道具' });
  }
  return t('videoGeneration.create.upload.subjectKindCharacter', { defaultValue: '角色' });
}
