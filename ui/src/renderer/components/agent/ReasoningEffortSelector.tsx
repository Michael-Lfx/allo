/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import type { ConversationId } from '@/common/types/ids';

import { ipcBridge } from '@/common';
import { useLayoutContext } from '@/renderer/hooks/context/LayoutContext';
import { useChatModelTriggerExpansion } from '@/renderer/components/model/useChatModelTriggerExpansion';
import {
  normalizeReasoningEffortLevels,
  pendingReasoningEffortCommitIndex,
  reasoningEffortAtIndex,
  reasoningEffortIndex,
  reasoningEffortProgress,
  reasoningEffortSliderViewModel,
} from '@/renderer/utils/model/reasoningEffort';
import { Popover, Slider } from '@arco-design/web-react';
import { AppMessage as Message } from '@/renderer/components/notifications';
import { Down, Lightning } from '@icon-park/react';
import classNames from 'classnames';
import React, {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  useId,
  type CSSProperties,
} from 'react';
import { useTranslation } from 'react-i18next';
import styles from './ReasoningEffortSelector.module.css';

export interface ReasoningEffortSelectorProps {
  /** When set, selection is persisted to conversation.extra.reasoning_effort. */
  conversation_id?: ConversationId;
  /** Catalog-advertised effort levels for the active chat model. Empty → hide. */
  levels: readonly string[];
  /** Identifies the active provider/model, including models with the same levels. */
  modelKey?: string;
  /** Current / preferred effort (may be stale vs levels). */
  initialEffort?: string;
  /** Current Nomi turn state. Changes are queued for the next turn. */
  isProcessing?: boolean;
  disabled?: boolean;
  className?: string;
  popupVisible?: boolean;
  onPopupVisibleChange?: (visible: boolean) => void;
  /** Fired after a successful local change (and after conversation persist when applicable). */
  onEffortChanged?: (effort: string | undefined) => void;
}

interface ReasoningSliderSurfaceProps {
  levels: readonly string[];
  index: number;
  progress: number;
  ariaLabel: string;
  statusId: string;
  disabled: boolean;
  maximumActive: boolean;
  isDragging: boolean;
  testId: string;
  onChange: (value: number | number[]) => void;
  onAfterChange: (value: number | number[]) => void;
  onPointerDown: () => void;
  onPointerCancel: () => void;
};

type ReasoningStatus = 'idle' | 'updated' | 'nextTurn' | 'failed';

const MAXIMUM_CHARGE_PARTICLES = [
  { delay: '0ms', offset: '-5px', size: '3px', opacity: '0.46' },
  { delay: '-320ms', offset: '-2px', size: '2px', opacity: '0.32' },
  { delay: '-670ms', offset: '3px', size: '4px', opacity: '0.5' },
  { delay: '-1010ms', offset: '5px', size: '2px', opacity: '0.28' },
  { delay: '-1380ms', offset: '0px', size: '3px', opacity: '0.38' },
] as const;

const ReasoningSliderSurface: React.FC<ReasoningSliderSurfaceProps> = ({
  levels,
  index,
  progress,
  ariaLabel,
  statusId,
  disabled,
  maximumActive,
  isDragging,
  testId,
  onChange,
  onAfterChange,
  onPointerDown,
  onPointerCancel,
}) => {
  const sliderContainerRef = useRef<HTMLDivElement>(null);
  const currentLevel = levels[index] ?? levels[0] ?? '';
  const fillStyle = { '--effort-progress': progress } as CSSProperties;

  useEffect(() => {
    const sliderThumb = sliderContainerRef.current?.querySelector<HTMLElement>('[role="slider"]');
    if (!sliderThumb) return;
    sliderThumb.setAttribute('aria-label', ariaLabel);
    sliderThumb.setAttribute('aria-describedby', statusId);
    sliderThumb.setAttribute('aria-valuetext', currentLevel);
  }, [ariaLabel, currentLevel, statusId]);

  const track = (
    <div
      className={styles.track}
      style={fillStyle}
      aria-hidden='true'
    >
      <span className={styles.trackGradient} />
      <span className={styles.trackDimmer} />
      <span className={styles.trackFill} />
      <span className={styles.levelMarkers}>
        {levels.map((level, levelIndex) => (
          <span
            key={`${level}-${levelIndex}`}
            className={styles.levelMarker}
            style={{ left: `${reasoningEffortProgress(levelIndex, levels.length) * 100}%` }}
            data-active={levelIndex <= index ? 'true' : 'false'}
          />
        ))}
      </span>
      {maximumActive && (
        <span className={styles.maximumChargePulse}>
          {MAXIMUM_CHARGE_PARTICLES.map((particle, particleIndex) => (
            <span
              key={`${particle.delay}-${particleIndex}`}
              className={styles.chargeParticle}
              style={
                {
                  '--particle-delay': particle.delay,
                  '--particle-offset': particle.offset,
                  '--particle-size': particle.size,
                  '--particle-opacity': particle.opacity,
                } as CSSProperties
              }
            />
          ))}
        </span>
      )}
    </div>
  );

  if (levels.length <= 1) {
    return (
      <div className={styles.sliderSurface} data-testid={testId} data-static='true'>
        {track}
      </div>
    );
  }

  return (
    <div
      ref={sliderContainerRef}
      className={styles.sliderSurface}
      data-testid={testId}
      data-static='false'
      data-dragging={isDragging ? 'true' : 'false'}
      data-max-active={maximumActive ? 'true' : 'false'}
      onPointerDownCapture={onPointerDown}
      onPointerCancelCapture={onPointerCancel}
      onBlurCapture={(event) => {
        const relatedTarget = event.relatedTarget;
        if (relatedTarget instanceof Node && event.currentTarget.contains(relatedTarget)) return;
        onPointerCancel();
      }}
    >
      {track}
      <Slider
        className={styles.arcoSlider}
        min={0}
        max={levels.length - 1}
        step={1}
        value={index}
        showTicks={false}
        disabled={disabled}
        tooltipVisible={false}
        tooltipPosition='top'
        formatTooltip={(value) => reasoningEffortAtIndex(levels, Number(value)) ?? currentLevel}
        onChange={onChange}
        onAfterChange={onAfterChange}
      />
    </div>
  );
};

/**
 * Quiet Kinetic Charge control for catalog-advertised reasoning depth.
 *
 * The slider is intentionally discrete. Cloud owns the order and the actual
 * strings sent to the model; this component only supplies a tactile preview,
 * persistence, and a small next-turn status for live Nomi sessions.
 */
const ReasoningEffortSelector: React.FC<ReasoningEffortSelectorProps> = ({
  conversation_id,
  levels,
  modelKey,
  initialEffort,
  isProcessing = false,
  disabled = false,
  className,
  popupVisible: popupVisibleProp,
  onPopupVisibleChange,
  onEffortChanged,
}) => {
  const { t } = useTranslation();
  const layout = useLayoutContext();
  const isMobile = Boolean(layout?.isMobile);
  const rawLevelKey = levels.join('\0');
  const normalizedLevels = useMemo(() => normalizeReasoningEffortLevels(levels), [rawLevelKey]);
  const levelKey = normalizedLevels.join('\0');
  const contextKey = `${conversation_id ?? 'guid'}\0${modelKey ?? ''}\0${levelKey}`;
  const viewModel = useMemo(
    () => reasoningEffortSliderViewModel(normalizedLevels, initialEffort),
    [initialEffort, normalizedLevels]
  );
  const [confirmedEffort, setConfirmedEffort] = useState<string | undefined>(viewModel.effort);
  const [draftIndex, setDraftIndex] = useState(viewModel.index);
  const [isDragging, setIsDragging] = useState(false);
  const [isSaving, setIsSaving] = useState(false);
  const [pendingForNextTurn, setPendingForNextTurn] = useState(false);
  const [lastStatus, setLastStatus] = useState<ReasoningStatus>('idle');
  const [localPopoverVisible, setLocalPopoverVisible] = useState(false);
  const popoverVisible = popupVisibleProp ?? localPopoverVisible;
  const popupInstanceId = useId().replace(/:/g, '');

  const confirmedEffortRef = useRef<string | undefined>(viewModel.effort);
  const draftIndexRef = useRef(viewModel.index);
  const pendingCommitRef = useRef<number | null>(null);
  const savingRef = useRef(false);
  const contextKeyRef = useRef('');
  const contextGenerationRef = useRef(0);
  const commitGenerationRef = useRef(0);
  const pointerDownRef = useRef(false);
  const pointerCommitRef = useRef(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const previousPopoverVisibleRef = useRef(false);
  const restoreFocusOnCloseRef = useRef(false);
  const statusTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const reasoningLabel = t('conversation.reasoningEffort.ariaLabel', {
    defaultValue: '推理深度',
  });
  const reasoningHint = t('conversation.reasoningEffort.hint', {
    defaultValue: '点击打开并拖动调整推理深度',
  });
  const currentEffort = normalizedLevels[draftIndex] ?? confirmedEffort ?? viewModel.effort;
  const currentLabel = currentEffort ?? t('acp.config.default');
  const currentProgress = reasoningEffortProgress(draftIndex, normalizedLevels.length);
  const maximumEffort = normalizedLevels[normalizedLevels.length - 1];
  const maximumActive = Boolean(
    popoverVisible &&
      normalizedLevels.length > 1 &&
      !isDragging &&
      draftIndex === normalizedLevels.length - 1 &&
      confirmedEffort === maximumEffort
  );
  const statusId = `reasoning-effort-status-${conversation_id ?? 'guid'}-${popupInstanceId}`;
  const popoverId = `reasoning-effort-popover-${conversation_id ?? 'guid'}-${popupInstanceId}`;
  const {
    ref: strategyTriggerRef,
    style: strategyTriggerStyle,
    side: strategyTriggerSide,
  } = useChatModelTriggerExpansion({
    enabled: !disabled && normalizedLevels.length > 1,
    expandedWidth: 108,
    cssVariablePrefix: 'strategy',
    slotSelector: '.sendbox-strategy-slot',
    open: popoverVisible,
  });
  const setTriggerRef = useCallback(
    (element: HTMLButtonElement | null) => {
      triggerRef.current = element;
      strategyTriggerRef(element);
    },
    [strategyTriggerRef],
  );

  const setPopupVisibility = useCallback(
    (visible: boolean) => {
      if (popupVisibleProp === undefined) {
        setLocalPopoverVisible(visible);
      }
      onPopupVisibleChange?.(visible);
    },
    [onPopupVisibleChange, popupVisibleProp]
  );

  const statusText =
    pendingForNextTurn || lastStatus === 'nextTurn'
      ? t('conversation.reasoningEffort.nextTurn')
      : lastStatus === 'updated'
        ? t('conversation.reasoningEffort.updated')
        : lastStatus === 'failed'
          ? t('conversation.reasoningEffort.switchFailed')
          : '';
  const clearStatusTimer = useCallback(() => {
    if (statusTimerRef.current) {
      clearTimeout(statusTimerRef.current);
      statusTimerRef.current = null;
    }
  }, []);

  const showAppliedStatus = useCallback(() => {
    clearStatusTimer();
    setLastStatus('updated');
    statusTimerRef.current = setTimeout(() => {
      setLastStatus('idle');
      statusTimerRef.current = null;
    }, 900);
  }, [clearStatusTimer]);

  useEffect(
    () => () => {
      clearStatusTimer();
    },
    [clearStatusTimer]
  );

  const persistEffort = useCallback(
    async (effort: string) => {
      if (!conversation_id) {
        return true;
      }
      const ok = await ipcBridge.conversation.update.invoke({
        conversation_id,
        updates: { extra: { reasoning_effort: effort } },
      });
      return Boolean(ok);
    },
    [conversation_id]
  );

  const flushCommit = useCallback(async () => {
    if (savingRef.current) return;
    const nextIndex = pendingCommitRef.current;
    if (nextIndex === null) return;

    pendingCommitRef.current = null;
    const effort = reasoningEffortAtIndex(normalizedLevels, nextIndex);
    if (!effort) return;

    const contextGeneration = contextGenerationRef.current;
    const requestGeneration = ++commitGenerationRef.current;
    const previousEffort = confirmedEffortRef.current;
    savingRef.current = true;
    setIsSaving(true);

    try {
      const ok = await persistEffort(effort);
      const isCurrentRequest =
        contextGeneration === contextGenerationRef.current && requestGeneration === commitGenerationRef.current;
      if (!isCurrentRequest) return;

      if (!ok) {
        clearStatusTimer();
        if (pendingCommitRef.current === null) {
          const previousIndex = reasoningEffortIndex(normalizedLevels, previousEffort);
          draftIndexRef.current = previousIndex;
          setDraftIndex(previousIndex);
        }
        setLastStatus('failed');
        Message.error(t('conversation.reasoningEffort.switchFailed'));
        return;
      }

      onEffortChanged?.(effort);
      confirmedEffortRef.current = effort;
      setConfirmedEffort(effort);
      if (isProcessing) {
        clearStatusTimer();
        setLastStatus('nextTurn');
        setPendingForNextTurn(true);
      } else {
        setPendingForNextTurn(false);
        showAppliedStatus();
      }

    } catch (error) {
      const isCurrentRequest =
        contextGeneration === contextGenerationRef.current && requestGeneration === commitGenerationRef.current;
      if (!isCurrentRequest) return;
      if (pendingCommitRef.current === null) {
        const previousIndex = reasoningEffortIndex(normalizedLevels, previousEffort);
        draftIndexRef.current = previousIndex;
        setDraftIndex(previousIndex);
      }
      clearStatusTimer();
      console.error('[ReasoningEffortSelector] Failed to update effort:', error);
      setLastStatus('failed');
      Message.error(t('conversation.reasoningEffort.switchFailed'));
    } finally {
      const isCurrentRequest =
        contextGeneration === contextGenerationRef.current && requestGeneration === commitGenerationRef.current;
      if (!isCurrentRequest) return;
      savingRef.current = false;
      setIsSaving(false);
      const pendingIndex = pendingCommitRef.current;
      const confirmedIndex = reasoningEffortIndex(normalizedLevels, confirmedEffortRef.current);
      if (pendingIndex === confirmedIndex) {
        pendingCommitRef.current = null;
      } else if (pendingIndex !== null) {
        void flushCommit();
      }
    }
  }, [
    clearStatusTimer,
    isProcessing,
    normalizedLevels,
    onEffortChanged,
    persistEffort,
    showAppliedStatus,
    t,
  ]);

  const queueCommit = useCallback(
    (nextIndex: number) => {
      const effort = reasoningEffortAtIndex(normalizedLevels, nextIndex);
      if (!effort) return;
      const confirmedIndex = reasoningEffortIndex(normalizedLevels, confirmedEffortRef.current);
      const pendingIndex = pendingCommitRef.current;
      const clampedIndex = reasoningEffortIndex(normalizedLevels, effort);

      draftIndexRef.current = clampedIndex;
      setDraftIndex(clampedIndex);
      if (savingRef.current) {
        pendingCommitRef.current = pendingReasoningEffortCommitIndex(
          clampedIndex,
          confirmedIndex,
          true
        );
        return;
      }
      if (effort === confirmedEffortRef.current && pendingIndex === null) return;
      if (clampedIndex === confirmedIndex) {
        pendingCommitRef.current = null;
        return;
      }
      if (pendingIndex === clampedIndex) return;

      pendingCommitRef.current = clampedIndex;
      if (!savingRef.current) void flushCommit();
    },
    [flushCommit, normalizedLevels]
  );

  const cancelPointerInteraction = useCallback(() => {
    if (!pointerDownRef.current) return;
    pointerDownRef.current = false;
    pointerCommitRef.current = true;
    setIsDragging(false);
    const confirmedIndex = reasoningEffortIndex(normalizedLevels, confirmedEffortRef.current);
    pendingCommitRef.current = savingRef.current ? confirmedIndex : null;
    draftIndexRef.current = confirmedIndex;
    setDraftIndex(confirmedIndex);
  }, [normalizedLevels]);

  const releasePointerInteraction = useCallback(() => {
    if (!pointerDownRef.current) return;
    pointerDownRef.current = false;
    setIsDragging(false);
    if (!pointerCommitRef.current) {
      pointerCommitRef.current = true;
      queueCommit(draftIndexRef.current);
    }
  }, [queueCommit]);

  useEffect(() => {
    window.addEventListener('pointerup', releasePointerInteraction);
    window.addEventListener('pointercancel', cancelPointerInteraction);
    return () => {
      window.removeEventListener('pointerup', releasePointerInteraction);
      window.removeEventListener('pointercancel', cancelPointerInteraction);
    };
  }, [cancelPointerInteraction, releasePointerInteraction]);

  useEffect(() => {
    if (contextKeyRef.current !== contextKey) {
      contextKeyRef.current = contextKey;
      contextGenerationRef.current += 1;
      commitGenerationRef.current += 1;
      pendingCommitRef.current = null;
      pointerDownRef.current = false;
      pointerCommitRef.current = false;
      savingRef.current = false;
      setIsSaving(false);
      setPendingForNextTurn(false);
      setLastStatus('idle');
      clearStatusTimer();
      setPopupVisibility(false);
    }

    const contextGeneration = contextGenerationRef.current;
    const healedEffort = viewModel.effort;
    confirmedEffortRef.current = healedEffort;
    setConfirmedEffort(healedEffort);
    draftIndexRef.current = viewModel.index;
    setDraftIndex(viewModel.index);
    if (!healedEffort || initialEffort === healedEffort) return;

    const requestGeneration = ++commitGenerationRef.current;
    savingRef.current = true;
    setIsSaving(true);
    void persistEffort(healedEffort)
      .then((ok) => {
        if (
          contextGeneration !== contextGenerationRef.current ||
          requestGeneration !== commitGenerationRef.current
        ) {
          return;
        }
        if (ok) {
          onEffortChanged?.(healedEffort);
          confirmedEffortRef.current = healedEffort;
          setConfirmedEffort(healedEffort);
        }
      })
      .catch(() => {
        // Healing an old value is best effort. The control still shows the
        // legal local fallback and retries when the conversation is reloaded.
      })
      .finally(() => {
        if (
          contextGeneration !== contextGenerationRef.current ||
          requestGeneration !== commitGenerationRef.current
        ) {
          return;
        }
        savingRef.current = false;
        setIsSaving(false);
        if (pendingCommitRef.current !== null) void flushCommit();
      });
    // `persistEffort` and `flushCommit` are stable for one conversation/model
    // context. Deliberately react to the catalog/value identity only.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [clearStatusTimer, contextKey, initialEffort, setPopupVisibility, viewModel.effort, viewModel.index]);

  useEffect(() => {
    if (!isProcessing) {
      setPendingForNextTurn(false);
      setLastStatus((status) => (status === 'nextTurn' ? 'idle' : status));
    }
  }, [isProcessing]);

  useEffect(() => {
    if (previousPopoverVisibleRef.current && !popoverVisible && restoreFocusOnCloseRef.current) {
      window.requestAnimationFrame(() => triggerRef.current?.focus());
    }
    restoreFocusOnCloseRef.current = false;
    previousPopoverVisibleRef.current = popoverVisible;
  }, [popoverVisible]);

  if (normalizedLevels.length === 0) return null;

  const handleSliderChange = (value: number | number[]) => {
    const nextIndex = typeof value === 'number' ? value : value[value.length - 1];
    if (typeof nextIndex !== 'number') return;
    const clampedIndex = reasoningEffortIndex(normalizedLevels, reasoningEffortAtIndex(normalizedLevels, nextIndex));
    draftIndexRef.current = clampedIndex;
    setDraftIndex(clampedIndex);
    if (!pointerDownRef.current) {
      pointerCommitRef.current = false;
      queueCommit(clampedIndex);
    }
  };

  const handleSliderDone = (value: number | number[]) => {
    const nextIndex = typeof value === 'number' ? value : value[value.length - 1];
    if (typeof nextIndex !== 'number') return;
    const clampedIndex = reasoningEffortIndex(normalizedLevels, reasoningEffortAtIndex(normalizedLevels, nextIndex));
    draftIndexRef.current = clampedIndex;
    setDraftIndex(clampedIndex);
    if (pointerDownRef.current) {
      if (pointerCommitRef.current) return;
      pointerCommitRef.current = true;
      queueCommit(clampedIndex);
      return;
    }
    if (!pointerCommitRef.current) queueCommit(clampedIndex);
    pointerCommitRef.current = false;
  };

  const handlePointerDown = () => {
    if (disabled || normalizedLevels.length <= 1) return;
    pointerDownRef.current = true;
    pointerCommitRef.current = false;
    setIsDragging(true);
  };

  const handlePopoverVisibleChange = (visible: boolean) => {
    if (!visible && pointerDownRef.current) cancelPointerInteraction();
    restoreFocusOnCloseRef.current = !visible;
    setPopupVisibility(visible);
  };

  const sliderProps = {
    levels: normalizedLevels,
    index: draftIndex,
    progress: currentProgress,
    ariaLabel: reasoningLabel,
    statusId,
    disabled,
    maximumActive,
    isDragging,
    onChange: handleSliderChange,
    onAfterChange: handleSliderDone,
    onPointerDown: handlePointerDown,
    onPointerCancel: cancelPointerInteraction,
  };

  if (normalizedLevels.length === 1) {
    return (
      <div
        className={classNames(styles.root, 'sendbox-responsive-strategy-root', className)}
        data-testid='reasoning-effort-selector'
        data-static='true'
        data-saving={isSaving ? 'true' : 'false'}
      >
        <span
          className={classNames(styles.staticValue, 'sendbox-responsive-static-value')}
          aria-label={`${reasoningLabel}: ${currentLabel}`}
          title={reasoningHint}
        >
          <span className='sendbox-responsive-leading-icon' data-layout-part='leading-icon'>
            <Lightning theme='filled' size='14' fill='currentColor' aria-hidden='true' />
          </span>
          <span>{currentLabel}</span>
        </span>
        <span id={statusId} className={styles.srOnly} aria-live='polite'>
          {statusText}
        </span>
      </div>
    );
  }

  const slider = <ReasoningSliderSurface {...sliderProps} testId='reasoning-effort-popover-slider' />;

  const popoverPanel = (
    <div
      id={popoverId}
      className={styles.popoverPanel}
      role='dialog'
      aria-label={reasoningLabel}
      data-saving={isSaving ? 'true' : 'false'}
      onClick={(event) => event.stopPropagation()}
    >
      {slider}
    </div>
  );

  return (
    <div
      className={classNames(styles.root, 'sendbox-responsive-strategy-root', className)}
      data-testid='reasoning-effort-selector'
      data-dragging={isDragging ? 'true' : 'false'}
      data-saving={isSaving ? 'true' : 'false'}
    >
      <Popover
        className='reasoning-effort-popover'
        trigger='click'
        position={isMobile ? 'top' : 'bottom'}
        content={popoverPanel}
        getPopupContainer={() => document.body}
        popupVisible={popoverVisible}
        onVisibleChange={handlePopoverVisibleChange}
        unmountOnExit
      >
        <button
          type='button'
          ref={setTriggerRef}
          style={strategyTriggerStyle}
          className={classNames(
            styles.compactTrigger,
            'sendbox-responsive-reasoning-btn',
            popoverVisible && 'sendbox-responsive-control-open',
          )}
          disabled={disabled}
          aria-label={`${reasoningLabel}: ${currentLabel}`}
          aria-haspopup='dialog'
          aria-controls={popoverId}
          aria-expanded={popoverVisible}
          aria-busy={isSaving || undefined}
          title={reasoningHint}
          data-chat-strategy-expand-side={strategyTriggerSide}
          data-testid='reasoning-effort-compact-trigger'
        >
          <span className='sendbox-responsive-leading-icon' data-layout-part='leading-icon'>
            <Lightning
              className={classNames(styles.triggerIcon, 'shrink-0')}
              theme='filled'
              size='14'
              fill='currentColor'
              aria-hidden='true'
            />
          </span>
          <span className={classNames(styles.triggerLabelSlot, 'sendbox-responsive-label')} aria-hidden='true'>
            {normalizedLevels.map((level) => (
              <span key={level} className={styles.triggerLabelReserve}>
                {level}
              </span>
            ))}
            <span className={styles.triggerLabelCurrent}>{currentLabel}</span>
          </span>
          <span className='sendbox-responsive-chevron-slot' data-layout-part='chevron'>
            <Down
              className={classNames(styles.triggerChevron, 'sendbox-responsive-chevron')}
              theme='outline'
              size='11'
              fill='currentColor'
              aria-hidden='true'
            />
          </span>
        </button>
      </Popover>

      <span id={statusId} className={styles.srOnly} aria-live='polite'>
        {statusText}
      </span>
    </div>
  );
};

export default ReasoningEffortSelector;
