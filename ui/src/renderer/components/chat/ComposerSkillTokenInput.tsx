import { getActiveSlashTokenRange } from '@/common/chat/slash/launcher';
import { Cube } from '@icon-park/react';
import React, {
  forwardRef,
  useCallback,
  useEffect,
  useImperativeHandle,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import type { ComposerSkillChip } from './composerSkill';
import {
  COMPOSER_SKILL_ATOM,
  createComposerDraft,
  getComposerDocumentOffset,
  getComposerDraftLength,
  getComposerDraftProjection,
  getComposerDraftSkillChips,
  getComposerDraftText,
  getComposerEditableTextLength,
  getComposerPlainTextOffset,
  insertComposerSkillAtRange,
  normalizeComposerDraft,
  replaceComposerDraftRange,
  type ComposerDraft,
  type ComposerDraftNode,
  type ComposerDraftSelection,
} from './composerDraft';
import styles from './ComposerSkillTokenInput.module.css';

const ZERO_WIDTH_SPACE = '\u200B';
const SKILL_TOKEN_SELECTOR = '[data-composer-skill-token="true"]';

export interface ComposerTokenInputState {
  projection: string;
  selection: ComposerDraftSelection;
  textSelection: ComposerDraftSelection;
}

export interface ComposerSkillTokenInputHandle {
  clear: () => ComposerDraft;
  focus: () => void;
  focusAtTextOffset: (offset: number) => void;
  getDraft: () => ComposerDraft;
  insertSkillAtActiveSlash: (skill: ComposerSkillChip) => boolean;
  insertTextAtSelection: (text: string) => void;
  replaceActiveSlashToken: (replacement?: string) => boolean;
  replaceDocumentRange: (selection: ComposerDraftSelection, replacement: string) => void;
  replaceTextRange: (selection: ComposerDraftSelection, replacement: string) => void;
  restoreDraft: (draft: ComposerDraft) => void;
}

interface ComposerSkillTokenInputProps {
  autoFocus?: boolean;
  className?: string;
  dataTestId?: string;
  disabled?: boolean;
  onBlur?: React.FocusEventHandler<HTMLDivElement>;
  onChange: (value: string) => void;
  onClick?: React.MouseEventHandler<HTMLDivElement>;
  onCompositionEndCapture?: React.CompositionEventHandler<HTMLDivElement>;
  onCompositionStartCapture?: React.CompositionEventHandler<HTMLDivElement>;
  onDraftStateChange?: (state: ComposerTokenInputState) => void;
  onFocus?: React.FocusEventHandler<HTMLDivElement>;
  onKeyDown?: React.KeyboardEventHandler<HTMLDivElement>;
  onMouseDown?: React.MouseEventHandler<HTMLDivElement>;
  onPaste?: React.ClipboardEventHandler<HTMLDivElement>;
  onScroll?: React.UIEventHandler<HTMLDivElement>;
  onSkillsChange?: (skills: ComposerSkillChip[]) => void;
  placeholder?: string;
  singleLine?: boolean;
  skills?: ComposerSkillChip[];
  style?: React.CSSProperties;
  value: string;
}

type ComposerPasteEvent = Pick<
  React.ClipboardEvent<HTMLDivElement>,
  'clipboardData' | 'defaultPrevented' | 'preventDefault' | 'stopPropagation'
>;

function normalizePastedText(text: string): string {
  return text.replace(/\n\s*$/, '');
}

/**
 * Owns the Composer's paste decision so the event boundary can be tested
 * without mounting a browser-only contentEditable tree.
 */
export function handleComposerPasteEvent(
  event: ComposerPasteEvent,
  replaceSelectionWithText: (text: string) => void,
  delegateToPasteService?: () => void
): void {
  if (event.defaultPrevented) {
    return;
  }

  const clipboardFiles = event.clipboardData.files;
  const text = event.clipboardData.getData('text/plain');
  if (clipboardFiles.length === 0 && text) {
    event.preventDefault();
    event.stopPropagation();
    replaceSelectionWithText(normalizePastedText(text));
    return;
  }

  delegateToPasteService?.();
}

function areSkillListsEqual(left: ComposerSkillChip[], right: ComposerSkillChip[]): boolean {
  return left.length === right.length && left.every((skill, index) => skill.skillId === right[index]?.skillId);
}

function buildState(draft: ComposerDraft, selection: ComposerDraftSelection): ComposerTokenInputState {
  return {
    projection: getComposerDraftProjection(draft),
    selection,
    textSelection: {
      start: getComposerPlainTextOffset(draft, selection.start),
      end: getComposerPlainTextOffset(draft, selection.end),
    },
  };
}

function getNodeDocumentLength(node: Node): number {
  if (node.nodeType === Node.TEXT_NODE) {
    return getComposerEditableTextLength(node.textContent ?? '');
  }

  if (!(node instanceof HTMLElement)) {
    return 0;
  }

  if (node.dataset.composerSkillToken === 'true') {
    return 1;
  }
  if (node.dataset.composerTokenGuard === 'true') {
    return getLogicalTextLength(node);
  }
  if (node.tagName === 'BR') {
    return 1;
  }

  return Array.from(node.childNodes).reduce((length, child) => length + getNodeDocumentLength(child), 0);
}

function getNodePlainTextLength(node: Node): number {
  if (node.nodeType === Node.TEXT_NODE) {
    return getComposerEditableTextLength(node.textContent ?? '');
  }

  if (!(node instanceof HTMLElement) || node.dataset.composerSkillToken === 'true') {
    return 0;
  }
  if (node.dataset.composerTokenGuard === 'true') {
    return getLogicalTextLength(node);
  }
  if (node.tagName === 'BR') {
    return 1;
  }

  return Array.from(node.childNodes).reduce((length, child) => length + getNodePlainTextLength(child), 0);
}

function getOffsetBeforePoint(root: HTMLElement, container: Node, offset: number, count: (node: Node) => number): number | null {
  if (!root.contains(container) && root !== container) {
    return null;
  }

  const range = document.createRange();
  range.selectNodeContents(root);
  try {
    range.setEnd(container, offset);
  } catch {
    return null;
  }

  return Array.from(range.cloneContents().childNodes).reduce((length, node) => length + count(node), 0);
}

function getDomSelection(root: HTMLElement, fallback: ComposerDraftSelection): ComposerDraftSelection {
  const selection = window.getSelection();
  if (!selection || selection.rangeCount === 0) {
    return fallback;
  }

  const range = selection.getRangeAt(0);
  const start = getOffsetBeforePoint(root, range.startContainer, range.startOffset, getNodeDocumentLength);
  const end = getOffsetBeforePoint(root, range.endContainer, range.endOffset, getNodeDocumentLength);
  if (start === null || end === null) {
    return fallback;
  }

  return { start, end };
}

function getLogicalTextLength(node: HTMLElement): number {
  return getComposerEditableTextLength(node.textContent ?? '');
}

function getDomTextOffset(node: HTMLElement, logicalOffset: number): number {
  const text = node.textContent ?? '';
  let remaining = logicalOffset;
  for (let index = 0; index < text.length; index += 1) {
    if (text[index] !== ZERO_WIDTH_SPACE) {
      if (remaining === 0) {
        return index;
      }
      remaining -= 1;
    }
  }
  return text.length;
}

function getPlaceholderInset(style: React.CSSProperties | undefined): string | undefined {
  const paddingStart = style?.paddingInlineStart ?? style?.paddingLeft;
  if (paddingStart === undefined) {
    return undefined;
  }

  const value = typeof paddingStart === 'number' ? `${paddingStart}px` : paddingStart;
  return `calc(${value} + 2px)`;
}

function setDomSelection(root: HTMLElement, selection: ComposerDraftSelection): void {
  const target = Math.max(0, Math.min(selection.end, getNodeDocumentLength(root)));
  let remaining = target;
  const children = Array.from(root.children) as HTMLElement[];
  const range = document.createRange();

  for (let index = 0; index < children.length; index += 1) {
    const child = children[index];
    if (child.dataset.composerSkillToken === 'true') {
      if (remaining === 0) {
        range.setStartBefore(child);
        range.collapse(true);
        window.getSelection()?.removeAllRanges();
        window.getSelection()?.addRange(range);
        return;
      }
      remaining -= 1;
      if (remaining === 0) {
        range.setStartAfter(child);
        range.collapse(true);
        window.getSelection()?.removeAllRanges();
        window.getSelection()?.addRange(range);
        return;
      }
      continue;
    }

    const length = getLogicalTextLength(child);
    if (remaining <= length) {
      const textNode = child.firstChild;
      if (textNode) {
        range.setStart(textNode, getDomTextOffset(child, remaining));
      } else {
        range.setStart(child, 0);
      }
      range.collapse(true);
      window.getSelection()?.removeAllRanges();
      window.getSelection()?.addRange(range);
      return;
    }
    remaining -= length;
  }

  range.selectNodeContents(root);
  range.collapse(false);
  window.getSelection()?.removeAllRanges();
  window.getSelection()?.addRange(range);
}

function readDraftFromDom(root: HTMLElement, currentDraft: ComposerDraft): ComposerDraft {
  const skills = new Map(getComposerDraftSkillChips(currentDraft).map((skill) => [skill.skillId, skill]));
  const nodes: ComposerDraftNode[] = [];

  const appendText = (text: string) => {
    const normalizedText = text.replaceAll(ZERO_WIDTH_SPACE, '');
    if (normalizedText) {
      nodes.push({ type: 'text', text: normalizedText });
    }
  };

  const visit = (node: Node) => {
    if (node.nodeType === Node.TEXT_NODE) {
      appendText(node.textContent ?? '');
      return;
    }
    if (!(node instanceof HTMLElement)) {
      return;
    }
    if (node.dataset.composerSkillToken === 'true') {
      const skill = skills.get(node.dataset.skillId ?? '');
      if (skill) {
        nodes.push({ type: 'skill', skill });
      }
      return;
    }
    if (node.dataset.composerTokenGuard === 'true') {
      appendText(node.textContent ?? '');
      return;
    }
    if (node.tagName === 'BR') {
      nodes.push({ type: 'text', text: '\n' });
      return;
    }
    Array.from(node.childNodes).forEach(visit);
  };

  Array.from(root.childNodes).forEach(visit);
  return normalizeComposerDraft(nodes);
}

const ComposerSkillTokenInput = forwardRef<ComposerSkillTokenInputHandle, ComposerSkillTokenInputProps>(
  (
    {
      autoFocus = false,
      className,
      dataTestId,
      disabled = false,
      onBlur,
      onChange,
      onClick,
      onCompositionEndCapture,
      onCompositionStartCapture,
      onDraftStateChange,
      onFocus,
      onKeyDown,
      onMouseDown,
      onPaste,
      onScroll,
      onSkillsChange,
      placeholder,
      singleLine = false,
      skills = [],
      style,
      value,
    },
    ref,
  ) => {
    const rootRef = useRef<HTMLDivElement>(null);
    const isComposingRef = useRef(false);
    const compositionFlushFrameRef = useRef<number | null>(null);
    const initialDraft = useMemo(() => createComposerDraft(value, skills), []);
    const [draft, setDraft] = useState<ComposerDraft>(initialDraft);
    const [isComposing, setIsComposing] = useState(false);
    const draftRef = useRef(draft);
    const selectionRef = useRef<ComposerDraftSelection>({
      start: getComposerDraftLength(initialDraft),
      end: getComposerDraftLength(initialDraft),
    });
    const pendingSelectionRef = useRef<ComposerDraftSelection | null>(selectionRef.current);

    const emitState = useCallback(
      (nextDraft: ComposerDraft, selection: ComposerDraftSelection) => {
        onDraftStateChange?.(buildState(nextDraft, selection));
      },
      [onDraftStateChange],
    );

    const applyDraft = useCallback(
      (nextDraft: ComposerDraft, selection: ComposerDraftSelection, publish = true) => {
        const normalizedDraft = normalizeComposerDraft(nextDraft);
        const documentLength = getComposerDraftLength(normalizedDraft);
        const nextSelection = {
          start: Math.max(0, Math.min(selection.start, documentLength)),
          end: Math.max(0, Math.min(selection.end, documentLength)),
        };

        draftRef.current = normalizedDraft;
        selectionRef.current = nextSelection;
        pendingSelectionRef.current = nextSelection;
        setDraft(normalizedDraft);
        if (publish) {
          onChange(getComposerDraftText(normalizedDraft));
          onSkillsChange?.(getComposerDraftSkillChips(normalizedDraft));
        }
        emitState(normalizedDraft, nextSelection);
      },
      [emitState, onChange, onSkillsChange],
    );

    useEffect(() => {
      const currentDraft = draftRef.current;
      if (getComposerDraftText(currentDraft) === value && areSkillListsEqual(getComposerDraftSkillChips(currentDraft), skills)) {
        return;
      }
      const nextDraft = createComposerDraft(value, skills);
      const end = getComposerDraftLength(nextDraft);
      applyDraft(nextDraft, { start: end, end }, false);
    }, [applyDraft, skills, value]);

    useEffect(
      () => () => {
        if (compositionFlushFrameRef.current !== null) {
          cancelAnimationFrame(compositionFlushFrameRef.current);
        }
      },
      [],
    );

    useLayoutEffect(() => {
      const pendingSelection = pendingSelectionRef.current;
      if (!pendingSelection || !rootRef.current) {
        return;
      }
      setDomSelection(rootRef.current, pendingSelection);
      pendingSelectionRef.current = null;
    }, [draft]);

    useEffect(() => {
      if (!autoFocus || !rootRef.current) {
        return;
      }
      rootRef.current.focus();
      setDomSelection(rootRef.current, selectionRef.current);
    }, [autoFocus]);

    const getLiveSelection = useCallback((): ComposerDraftSelection => {
      const selection = rootRef.current
        ? getDomSelection(rootRef.current, selectionRef.current)
        : selectionRef.current;
      selectionRef.current = selection;
      return selection;
    }, []);

    const syncSelection = useCallback(() => {
      if (isComposingRef.current) {
        return;
      }
      const selection = getLiveSelection();
      emitState(draftRef.current, selection);
    }, [emitState, getLiveSelection]);

    const moveCaret = useCallback(
      (offset: number) => {
        const documentLength = getComposerDraftLength(draftRef.current);
        const nextOffset = Math.max(0, Math.min(offset, documentLength));
        const selection = { start: nextOffset, end: nextOffset };
        selectionRef.current = selection;
        if (rootRef.current) {
          setDomSelection(rootRef.current, selection);
        }
        emitState(draftRef.current, selection);
      },
      [emitState],
    );

    const replaceSelectionWithText = useCallback(
      (text: string) => {
        const selection = getLiveSelection();
        const start = Math.min(selection.start, selection.end);
        const nextDraft = replaceComposerDraftRange(
          draftRef.current,
          selection,
          text ? [{ type: 'text', text }] : [],
        );
        const nextOffset = start + text.length;
        applyDraft(nextDraft, { start: nextOffset, end: nextOffset });
      },
      [applyDraft, getLiveSelection],
    );

    const deleteSelection = useCallback(
      (direction: 'backward' | 'forward') => {
        const selection = getLiveSelection();
        const collapsed = selection.start === selection.end;
        if (collapsed) {
          if (direction === 'backward' && selection.start === 0) {
            return false;
          }
          if (direction === 'forward' && selection.end >= getComposerDraftLength(draftRef.current)) {
            return false;
          }
        }

        const start = collapsed && direction === 'backward' ? selection.start - 1 : Math.min(selection.start, selection.end);
        const end = collapsed && direction === 'forward' ? selection.end + 1 : Math.max(selection.start, selection.end);
        const nextDraft = replaceComposerDraftRange(draftRef.current, { start, end }, []);
        applyDraft(nextDraft, { start, end: start });
        return true;
      },
      [applyDraft, getLiveSelection],
    );

    useImperativeHandle(
      ref,
      () => ({
        clear: () => {
          const cleared: ComposerDraft = [];
          applyDraft(cleared, { start: 0, end: 0 });
          return cleared;
        },
        focus: () => {
          rootRef.current?.focus();
          if (rootRef.current) {
            setDomSelection(rootRef.current, selectionRef.current);
          }
        },
        focusAtTextOffset: (offset) => {
          const documentOffset = getComposerDocumentOffset(draftRef.current, offset, true);
          const selection = { start: documentOffset, end: documentOffset };
          selectionRef.current = selection;
          rootRef.current?.focus();
          if (rootRef.current) {
            setDomSelection(rootRef.current, selection);
          }
          emitState(draftRef.current, selection);
        },
        getDraft: () => draftRef.current,
        insertSkillAtActiveSlash: (skill) => {
          const selection = getLiveSelection();
          const range = getActiveSlashTokenRange(getComposerDraftProjection(draftRef.current), selection.end);
          if (!range) {
            return false;
          }
          const alreadySelected = getComposerDraftSkillChips(draftRef.current).some(
            (candidate) => candidate.skillId === skill.skillId,
          );
          const nextDraft = insertComposerSkillAtRange(draftRef.current, range, skill);
          const nextOffset = range.start + (alreadySelected ? 0 : 1);
          rootRef.current?.focus();
          applyDraft(nextDraft, { start: nextOffset, end: nextOffset });
          return true;
        },
        insertTextAtSelection: (text) => {
          replaceSelectionWithText(text);
        },
        replaceActiveSlashToken: (replacement = '') => {
          const selection = getLiveSelection();
          const range = getActiveSlashTokenRange(getComposerDraftProjection(draftRef.current), selection.end);
          if (!range) {
            return false;
          }
          const nextDraft = replaceComposerDraftRange(
            draftRef.current,
            range,
            replacement ? [{ type: 'text', text: replacement }] : [],
          );
          const nextOffset = range.start + replacement.length;
          rootRef.current?.focus();
          applyDraft(nextDraft, { start: nextOffset, end: nextOffset });
          return true;
        },
        replaceDocumentRange: (selection, replacement) => {
          const nextDraft = replaceComposerDraftRange(
            draftRef.current,
            selection,
            replacement ? [{ type: 'text', text: replacement }] : [],
          );
          const nextOffset = Math.min(selection.start, selection.end) + replacement.length;
          applyDraft(nextDraft, { start: nextOffset, end: nextOffset });
        },
        replaceTextRange: (selection, replacement) => {
          const start = getComposerDocumentOffset(draftRef.current, selection.start, true);
          const end = getComposerDocumentOffset(draftRef.current, selection.end, true);
          const nextDraft = replaceComposerDraftRange(
            draftRef.current,
            { start, end },
            replacement ? [{ type: 'text', text: replacement }] : [],
          );
          const nextOffset = start + replacement.length;
          applyDraft(nextDraft, { start: nextOffset, end: nextOffset });
        },
        restoreDraft: (nextDraft) => {
          const end = getComposerDraftLength(nextDraft);
          applyDraft(nextDraft, { start: end, end });
        },
      }),
      [applyDraft, emitState, getLiveSelection, replaceSelectionWithText],
    );

    const syncDraftFromDom = useCallback(() => {
      if (!rootRef.current) {
        return;
      }
      const selection = getDomSelection(rootRef.current, selectionRef.current);
      const nextDraft = readDraftFromDom(rootRef.current, draftRef.current);
      applyDraft(nextDraft, selection);
    }, [applyDraft]);

    const handleCompositionStartCapture = (event: React.CompositionEvent<HTMLDivElement>) => {
      if (compositionFlushFrameRef.current !== null) {
        cancelAnimationFrame(compositionFlushFrameRef.current);
        compositionFlushFrameRef.current = null;
      }
      isComposingRef.current = true;
      setIsComposing(true);
      onCompositionStartCapture?.(event);
    };

    const handleCompositionEndCapture = (event: React.CompositionEvent<HTMLDivElement>) => {
      onCompositionEndCapture?.(event);
      // The final native input can arrive after compositionend. Keep React out
      // of the editable DOM for one frame, then reconcile the settled value.
      compositionFlushFrameRef.current = requestAnimationFrame(() => {
        compositionFlushFrameRef.current = null;
        isComposingRef.current = false;
        setIsComposing(false);
        syncDraftFromDom();
      });
    };

    const handleBeforeInput = (event: React.FormEvent<HTMLDivElement>) => {
      if (disabled) {
        return;
      }
      const nativeEvent = event.nativeEvent as InputEvent;
      if (isComposingRef.current || nativeEvent.isComposing || nativeEvent.inputType.includes('Composition')) {
        return;
      }

      switch (nativeEvent.inputType) {
        case 'insertText':
          if (nativeEvent.data !== null) {
            event.preventDefault();
            replaceSelectionWithText(nativeEvent.data);
          }
          break;
        case 'insertLineBreak':
        case 'insertParagraph':
          event.preventDefault();
          replaceSelectionWithText('\n');
          break;
        case 'deleteContentBackward':
          event.preventDefault();
          deleteSelection('backward');
          break;
        case 'deleteContentForward':
          event.preventDefault();
          deleteSelection('forward');
          break;
        default:
          break;
      }
    };

    const handleInput = (event: React.FormEvent<HTMLDivElement>) => {
      const nativeEvent = event.nativeEvent as InputEvent;
      if (isComposingRef.current || nativeEvent.isComposing) {
        return;
      }
      if (compositionFlushFrameRef.current !== null) {
        cancelAnimationFrame(compositionFlushFrameRef.current);
        compositionFlushFrameRef.current = null;
      }
      syncDraftFromDom();
    };

    const handleKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
      onKeyDown?.(event);
      if (
        event.defaultPrevented ||
        disabled ||
        isComposingRef.current ||
        event.nativeEvent.isComposing ||
        event.keyCode === 229
      ) {
        return;
      }

      if (event.key === 'Backspace') {
        syncSelection();
        deleteSelection('backward');
        // The zero-width guard is React-owned. Never let the browser remove
        // it when the caret is already at the document boundary.
        event.preventDefault();
        return;
      }
      if (event.key === 'Delete') {
        syncSelection();
        deleteSelection('forward');
        event.preventDefault();
        return;
      }

      if (!event.shiftKey && !event.metaKey && !event.ctrlKey && !event.altKey) {
        const selection = getLiveSelection();
        if (event.key === 'ArrowLeft') {
          event.preventDefault();
          moveCaret(selection.start === selection.end ? selection.start - 1 : Math.min(selection.start, selection.end));
        } else if (event.key === 'ArrowRight') {
          event.preventDefault();
          moveCaret(selection.start === selection.end ? selection.end + 1 : Math.max(selection.start, selection.end));
        }
      }
    };

    const handlePaste = (event: React.ClipboardEvent<HTMLDivElement>) => {
      if (disabled) {
        return;
      }
      handleComposerPasteEvent(event, replaceSelectionWithText, onPaste ? () => onPaste(event) : undefined);
    };

    const hasVisibleText = getComposerDraftText(draft).length > 0;
    const hasSkills = getComposerDraftSkillChips(draft).length > 0;
    const placeholderInset = getPlaceholderInset(style);
    const rootStyle = placeholderInset
      ? ({ ...style, '--composer-placeholder-inset': placeholderInset } as React.CSSProperties)
      : style;

    return (
      <div
        ref={rootRef}
        className={`${styles.root} ${singleLine ? styles.singleLine : ''} ${className ?? ''}`}
        style={rootStyle}
        role='textbox'
        aria-multiline={!singleLine}
        aria-label={placeholder}
        aria-disabled={disabled || undefined}
        contentEditable={!disabled}
        suppressContentEditableWarning
        spellCheck={false}
        data-empty={!hasVisibleText && !hasSkills && !isComposing ? 'true' : undefined}
        data-placeholder={placeholder}
        data-testid={dataTestId}
        onBeforeInput={handleBeforeInput}
        onInput={handleInput}
        onCompositionStartCapture={handleCompositionStartCapture}
        onCompositionEndCapture={handleCompositionEndCapture}
        onKeyDown={handleKeyDown}
        onKeyUp={syncSelection}
        onSelect={syncSelection}
        onClick={(event) => {
          syncSelection();
          onClick?.(event);
        }}
        onMouseDown={onMouseDown}
        onFocus={(event) => {
          syncSelection();
          onFocus?.(event);
        }}
        onBlur={onBlur}
        onPaste={handlePaste}
        onScroll={onScroll}
      >
        {draft.map((node, index) =>
          node.type === 'text' ? (
            <span key={`text-${index}`} data-composer-token-text='true'>
              {node.text}
            </span>
          ) : (
            <span
              key={`skill-${node.skill.skillId}`}
              className={styles.skill}
              contentEditable={false}
              data-composer-skill-token='true'
              data-skill-id={node.skill.skillId}
              title={node.skill.name}
            >
              <span className={styles.skillIcon} aria-hidden='true'>
                <Cube theme='outline' size={16} fill='currentColor' />
              </span>
              <span className={styles.skillName}>{node.skill.name}</span>
            </span>
          ),
        )}
        {draft.length === 0 || draft.at(-1)?.type === 'skill' ? (
          <span data-composer-token-guard='true'>{ZERO_WIDTH_SPACE}</span>
        ) : null}
      </div>
    );
  },
);

export { COMPOSER_SKILL_ATOM };
export default ComposerSkillTokenInput;
