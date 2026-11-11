/**
 * @license
 * Copyright 2025-2026 Flowy (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, {
  useCallback,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
} from 'react';
import { useTranslation } from 'react-i18next';
import {
  ArrowUpRight,
  AudioLines,
  ChevronDown,
  ChevronUp,
  LoaderCircle,
  Mic,
  MicOff,
  TriangleAlert,
  Volume2,
} from 'lucide-react';
import type { VoiceSnapshot } from '@common/adapter/voiceCompanion';
import type { TMessage } from '@common/chat/chatLib';
import type { ConversationId } from '@common/types/ids';
import VoiceCallDockDetails from './VoiceCallDockDetails';
import {
  resolveVoiceDockPresentation,
  type VoiceDockActivity,
  type VoiceDockTone,
} from './voiceCallDockState';
import { useVoiceTurnCredits } from './useVoiceTurnCredits';
import './voiceCallDock.css';

const WAVE_WEIGHTS = [0.55, 0.9, 1, 0.75, 0.45] as const;
const clamp = (value: number, min: number, max: number): number =>
  Math.min(max, Math.max(min, value));

function useDocumentVisible(): boolean {
  const [visible, setVisible] = useState(() =>
    typeof document === 'undefined' ? true : document.visibilityState !== 'hidden'
  );
  useEffect(() => {
    if (typeof document === 'undefined') return;
    const update = () => setVisible(document.visibilityState !== 'hidden');
    document.addEventListener('visibilitychange', update);
    return () => document.removeEventListener('visibilitychange', update);
  }, []);
  return visible;
}

function DockIcon({
  tone,
  activity,
  canInterrupt,
}: {
  tone: VoiceDockTone;
  activity: VoiceDockActivity;
  canInterrupt: boolean;
}) {
  if (tone === 'danger') return <MicOff size={18} strokeWidth={1.8} aria-hidden='true' />;
  if (tone === 'attention') return <TriangleAlert size={18} strokeWidth={1.8} aria-hidden='true' />;
  if (activity === 'speaker') return <Volume2 size={18} strokeWidth={1.8} aria-hidden='true' />;
  if (activity === 'wave') return <Mic size={18} strokeWidth={1.8} aria-hidden='true' />;
  if (activity === 'breathe') return <Mic size={18} strokeWidth={1.8} aria-hidden='true' />;
  if (activity === 'dots') return <LoaderCircle size={18} strokeWidth={1.8} aria-hidden='true' />;
  if (canInterrupt) return <Volume2 size={18} strokeWidth={1.8} aria-hidden='true' />;
  return <AudioLines size={18} strokeWidth={1.8} aria-hidden='true' />;
}

function DockActivity({
  activity,
  live,
  bars,
}: {
  activity: VoiceDockActivity;
  live: boolean;
  bars: readonly number[];
}) {
  if (activity === 'dots') {
    return (
      <span className='voice-call-dock__dots' aria-hidden='true'>
        <span />
        <span />
        <span />
      </span>
    );
  }
  if (activity === 'breathe') {
    return (
      <span className='voice-call-dock__breath' aria-hidden='true'>
        <span />
      </span>
    );
  }
  if (activity === 'wave' || activity === 'speaker') {
    return (
      <span
        className='voice-call-dock__wave'
        data-live={live ? 'true' : 'false'}
        aria-hidden='true'
      >
        {bars.map((height, index) => (
          <span
            key={index}
            style={
              live ? ({ '--voice-bar': String(height) } as React.CSSProperties) : undefined
            }
          />
        ))}
      </span>
    );
  }
  return null;
}

export default function VoiceCallDock({
  conversationId,
  snapshot,
  failed = false,
  historyFailed = false,
  historyMessages,
  isConnecting = false,
  isActiveElsewhere = false,
  onRetryHistory,
  onReturnToCall,
}: {
  conversationId: ConversationId | string;
  snapshot: VoiceSnapshot | null;
  failed?: boolean;
  historyFailed?: boolean;
  historyMessages?: readonly TMessage[];
  isConnecting?: boolean;
  isActiveElsewhere?: boolean;
  onRetryHistory?: () => Promise<void>;
  onReturnToCall?: () => void;
}) {
  const { t } = useTranslation();
  const panelId = useId();
  const [detailsOpen, setDetailsOpen] = useState(false);
  const toggleRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const visible = useDocumentVisible();
  useVoiceTurnCredits(conversationId, snapshot, historyMessages);

  const presentation = useMemo(
    () =>
      resolveVoiceDockPresentation({
        conversationId,
        snapshot,
        failed,
        historyFailed,
        isConnecting,
        isActiveElsewhere,
        hasReturnAction: Boolean(onReturnToCall),
      }),
    [
      conversationId,
      failed,
      historyFailed,
      isActiveElsewhere,
      isConnecting,
      onReturnToCall,
      snapshot,
    ]
  );

  const liveRms =
    presentation.activity === 'wave' &&
    snapshot?.audio?.capture_state === 'receiving' &&
    snapshot.audio.speech_detected
      ? clamp(snapshot.audio.rms_milli / 1000, 0, 1)
      : 0;
  const bars = useMemo(
    () =>
      WAVE_WEIGHTS.map((weight) =>
        Number(clamp(0.28 + liveRms * weight, 0.28, 1).toFixed(2))
      ),
    [liveRms]
  );

  const closeDetails = useCallback((restoreFocus = true) => {
    setDetailsOpen(false);
    if (restoreFocus) {
      requestAnimationFrame(() => {
        toggleRef.current?.focus({ preventScroll: true });
      });
    }
  }, []);

  useEffect(() => {
    setDetailsOpen(false);
  }, [conversationId]);

  useEffect(() => {
    if (!presentation.hasDetails) closeDetails(false);
  }, [closeDetails, presentation.hasDetails]);

  useEffect(() => {
    if (!detailsOpen) return;
    panelRef.current?.focus({ preventScroll: true });
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      event.preventDefault();
      event.stopPropagation();
      closeDetails();
    };
    document.addEventListener('keydown', onKeyDown);
    return () => document.removeEventListener('keydown', onKeyDown);
  }, [closeDetails, detailsOpen]);

  const label = t(presentation.labelKey, { defaultValue: presentation.labelFallback });
  const detailsCount =
    presentation.details.queue.length +
    (presentation.needsApproval ? 1 : 0) +
    (presentation.syncFailed ? 1 : 0) +
    (presentation.details.error ? 1 : 0);
  const detailsLabel = presentation.needsApproval
    ? t('voice.dock.reviewApproval', { defaultValue: 'Review approval' })
    : detailsCount > 0
      ? t('voice.dock.detailsWithCount', {
          count: detailsCount,
          defaultValue: 'Details ({{count}})',
        })
      : t('voice.dock.details', { defaultValue: 'Details' });

  return (
    <div
      className='voice-call-dock'
      data-testid='voice-call-dock'
      data-voice-tone={presentation.tone}
      data-voice-activity={presentation.activity}
      data-voice-phase={snapshot?.phase ?? 'unknown'}
      data-page-hidden={visible ? 'false' : 'true'}
    >
      <div className='voice-call-dock__row'>
        <div className='voice-call-dock__bar' data-testid='voice-call-dock-bar'>
          <span className='voice-call-dock__icon'>
            <DockIcon
              tone={presentation.tone}
              activity={presentation.activity}
              canInterrupt={presentation.canInterrupt}
            />
          </span>
          <DockActivity
            activity={presentation.activity}
            live={liveRms > 0}
            bars={bars}
          />
          <span
            key={presentation.key}
            className='voice-call-dock__label'
            data-testid='voice-call-dock-status'
            role={presentation.alert ? 'alert' : 'status'}
            aria-live={presentation.alert ? undefined : 'polite'}
            aria-atomic='true'
          >
            {label}
          </span>
          {presentation.canInterrupt && (
            <span className='voice-call-dock__interrupt-hint' aria-hidden='true'>
              <Mic size={14} strokeWidth={2} />
            </span>
          )}
          {presentation.taskRunning && (
            <span className='voice-call-dock__badge voice-call-dock__aux'>
              {t('voice.dock.taskRunning', { defaultValue: 'Task running' })}
            </span>
          )}
          {presentation.syncFailed && (
            <span
              className='voice-call-dock__sync-warning voice-call-dock__aux'
              aria-label={t('voice.dock.syncFailed', {
                defaultValue: 'History sync failed',
              })}
              title={t('voice.dock.syncFailed', {
                defaultValue: 'History sync failed',
              })}
            >
              <TriangleAlert size={14} strokeWidth={1.8} aria-hidden='true' />
            </span>
          )}
          {presentation.showReturn && onReturnToCall && (
            <button
              type='button'
              className='voice-call-dock__action voice-call-dock__return'
              data-testid='voice-call-dock-return'
              onClick={onReturnToCall}
            >
              <ArrowUpRight size={14} strokeWidth={1.9} aria-hidden='true' />
              <span>{t('voice.entry.returnVoiceCall')}</span>
            </button>
          )}
          {presentation.hasDetails && (
            <button
              type='button'
              ref={toggleRef}
              className='voice-call-dock__action voice-call-dock__details-toggle'
              data-testid='voice-call-dock-details-toggle'
              aria-expanded={detailsOpen}
              aria-controls={panelId}
              onClick={() => {
                if (detailsOpen) closeDetails();
                else setDetailsOpen(true);
              }}
            >
              <span>{detailsOpen ? t('voice.dock.hideDetails', { defaultValue: 'Hide details' }) : detailsLabel}</span>
              {detailsOpen ? (
                <ChevronDown size={14} strokeWidth={1.9} aria-hidden='true' />
              ) : (
                <ChevronUp size={14} strokeWidth={1.9} aria-hidden='true' />
              )}
            </button>
          )}
        </div>
      </div>
      {presentation.hasDetails && (
        <div
          id={panelId}
          ref={panelRef}
          className='voice-call-dock__panel'
          data-testid='voice-call-dock-details'
          role='region'
          aria-label={t('voice.dock.detailsAria', {
            defaultValue: 'Voice call details',
          })}
          tabIndex={-1}
          hidden={!detailsOpen}
        >
          {detailsOpen && (
            <VoiceCallDockDetails
              conversationId={conversationId}
              snapshot={snapshot}
              details={presentation.details}
              onRetryHistory={onRetryHistory}
            />
          )}
        </div>
      )}
    </div>
  );
}
