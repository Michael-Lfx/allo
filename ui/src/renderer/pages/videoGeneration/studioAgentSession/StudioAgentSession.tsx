import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, Spin, Tag } from '@arco-design/web-react';
import { CloseSmall, Down, Robot, Up } from '@icon-park/react';
import { classifyFailure, type FailureKind } from '../classifyFailure';
import { formatElapsedClock } from '../progressEventElapsed';
import { stageLabel } from '../stageI18n';
import { statusLabel, statusTagColor } from '../components/SessionCard';
import {
  buildStudioStageTimeline,
  type StudioStageKey,
  type StudioStageVariant,
} from '../studioStageTimeline';
import { useDocumentHidden, useRunStatusFull } from '../useRunStatusFeed';
import { resolveSessionCreditsConsumed } from '../sessionCredits';
import { listCameos } from '../api';
import type { ArtifactNode, CameoPhoto } from '../types';
import StudioMediaLightbox from './StudioMediaLightbox';
import StudioSessionComposer from './StudioSessionComposer';
import StudioSessionMessageView from './StudioSessionMessage';
import {
  projectStudioSessionMessages,
  resolveStudioComposerAction,
} from './projectStudioSessionMessages';
import { collectCameoMedia, collectSourceDocumentMedia } from './collectStudioMedia';
import { clampStudioSessionWidth } from './sessionPanelStorage';
import type { StudioSessionMedia } from './types';
import styles from './index.module.css';

const STAGE_LABEL: Record<StudioStageKey, { key: string; fallback: string }> = {
  brief: { key: 'videoGeneration.studio.stages.brief', fallback: '创意' },
  storyboard: { key: 'videoGeneration.studio.stages.storyboard', fallback: '分镜' },
  render: { key: 'videoGeneration.studio.stages.render', fallback: '渲染' },
  film: { key: 'videoGeneration.studio.stages.film', fallback: '成片' },
  assets: { key: 'videoGeneration.studio.stages.assets', fallback: '素材' },
  generate: { key: 'videoGeneration.studio.stages.generate', fallback: '生成' },
};

export interface StudioAgentSessionProps {
  sessionId: string;
  artifacts: ArtifactNode[];
  sourceText: string;
  hasStoryboard: boolean;
  hasFinalVideo: boolean;
  coverPath?: string | null;
  finalVideoPath?: string | null;
  isAction: boolean;
  actionAssetsReady: boolean;
  canRender: boolean;
  isFailed: boolean;
  busy: boolean;
  planning?: boolean;
  rendering?: boolean;
  cancelling?: boolean;
  creditsConsumed?: number;
  models?: { llm_model?: string; image_model?: string; video_model?: string };
  isMobile: boolean;
  collapsed: boolean;
  width: number;
  onCollapsedChange: (collapsed: boolean) => void;
  onWidthChange: (width: number) => void;
  onPlan: () => void;
  onRender: () => void;
  onCancel: () => void;
  onContinue: () => void;
  onFocusScene?: (sceneId: string) => void;
  onSelectArtifact?: (path: string) => void;
  cameoEpoch?: number;
  sourceDocumentName?: string | null;
}

const StudioAgentSession: React.FC<StudioAgentSessionProps> = ({
  sessionId,
  artifacts,
  sourceText,
  hasStoryboard,
  hasFinalVideo,
  coverPath,
  finalVideoPath,
  isAction,
  actionAssetsReady,
  canRender,
  isFailed,
  busy,
  planning,
  rendering,
  cancelling,
  creditsConsumed,
  models,
  isMobile,
  collapsed,
  width,
  onCollapsedChange,
  onWidthChange,
  onPlan,
  onRender,
  onCancel,
  onContinue,
  onFocusScene,
  onSelectArtifact,
  cameoEpoch = 0,
  sourceDocumentName = null,
}) => {
  const { t } = useTranslation();
  const runStatus = useRunStatusFull();
  const hidden = useDocumentHidden();
  const scrollerRef = useRef<HTMLDivElement>(null);
  const columnRef = useRef<HTMLDivElement>(null);
  const [nowMs, setNowMs] = useState(() => Date.now());
  const [cameos, setCameos] = useState<CameoPhoto[]>([]);
  const [lightbox, setLightbox] = useState<{ items: StudioSessionMedia[]; index: number } | null>(
    null
  );

  const variant: StudioStageVariant = isAction ? 'action' : 'film';
  const liveBusy = busy || runStatus?.status === 'planning' || runStatus?.status === 'rendering';

  useEffect(() => {
    if (isAction || !sessionId) {
      setCameos([]);
      return;
    }
    let cancelled = false;
    void listCameos(sessionId)
      .then((photos) => {
        if (!cancelled) setCameos(photos);
      })
      .catch(() => {
        if (!cancelled) setCameos([]);
      });
    return () => {
      cancelled = true;
    };
  }, [sessionId, isAction, cameoEpoch]);

  const briefMedia = useMemo(
    () => [...collectCameoMedia(cameos), ...collectSourceDocumentMedia(sourceDocumentName)],
    [cameos, sourceDocumentName]
  );

  useEffect(() => {
    if (!liveBusy || hidden) return;
    setNowMs(Date.now());
    const id = window.setInterval(() => setNowMs(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [liveBusy, hidden]);

  const messages = useMemo(
    () =>
      projectStudioSessionMessages({
        sourceText,
        status: runStatus,
        artifacts,
        hasStoryboard,
        hasFinalVideo,
        coverPath,
        finalVideoPath,
        isAction,
        actionAssetsReady,
        variant,
        runStatus: runStatus?.status,
        briefMedia,
      }),
    [
      sourceText,
      runStatus,
      artifacts,
      hasStoryboard,
      hasFinalVideo,
      coverPath,
      finalVideoPath,
      isAction,
      actionAssetsReady,
      variant,
      briefMedia,
    ]
  );

  const action = resolveStudioComposerAction({
    busy: liveBusy,
    isFailed,
    isAction,
    hasStoryboard,
    hasFinalVideo,
    actionAssetsReady,
    canRender,
  });

  useEffect(() => {
    const node = scrollerRef.current;
    if (!node) return;
    node.scrollTop = node.scrollHeight;
  }, [messages.length, runStatus?.stage, runStatus?.progress]);

  const timeline = useMemo(
    () =>
      buildStudioStageTimeline({
        status: runStatus?.status,
        stage: runStatus?.stage,
        events: runStatus?.events,
        updatedAt: runStatus?.updated_at,
        nowMs,
        hasStoryboard,
        hasFinalVideo,
        variant,
      }),
    [runStatus, nowMs, hasStoryboard, hasFinalVideo, variant]
  );
  const activeSegment =
    timeline.find((seg) => seg.state === 'active' || seg.state === 'failed') ??
    [...timeline].reverse().find((seg) => seg.state === 'done');

  const liveCredits = resolveSessionCreditsConsumed({
    sessionCredits: creditsConsumed,
    statusCredits: runStatus?.credits_consumed,
    events: runStatus?.events,
  });
  const progress = Math.max(0, Math.min(100, Number(runStatus?.progress) || 0));

  const failure = useMemo(() => {
    if (!runStatus?.error) return null;
    return classifyFailure(runStatus.error, runStatus.stage, runStatus.events, t);
  }, [runStatus?.error, runStatus?.stage, runStatus?.events, t]);

  const staleHint = useMemo(() => {
    if (!runStatus || (runStatus.status !== 'planning' && runStatus.status !== 'rendering')) {
      return null;
    }
    const raw = runStatus.updated_at || runStatus.events?.[runStatus.events.length - 1]?.at;
    if (!raw) return null;
    const ts = Date.parse(raw);
    if (Number.isNaN(ts)) return null;
    if ((nowMs - ts) / 1000 < 90) return null;
    return t('videoGeneration.workspace.progress.stale');
  }, [runStatus, nowMs, t]);

  const relatedModel =
    failure?.kind === 'llm'
      ? models?.llm_model
      : failure?.kind === 'image'
        ? models?.image_model
        : failure?.kind === 'video' || failure?.kind === 'moderation'
          ? models?.video_model
          : undefined;

  const copyFor = useCallback(
    (item: (typeof messages)[number]): {
      title: string;
      body?: string;
      meta?: string;
      detail?: string;
      issueKind?: FailureKind;
    } => {
      if (item.kind === 'user_brief' || item.kind === 'user_note') {
        return { title: '', body: item.text };
      }
      if (item.kind === 'gate_render') {
        return {
          title: t('videoGeneration.agentSession.gate.renderTitle', {
            defaultValue: '分镜和定妆可以开渲了吗？',
          }),
          body: t('videoGeneration.agentSession.gate.renderBody', {
            defaultValue: '建议先在左侧检查分镜与定妆。确认后将逐镜出片并拼接，成本较高。',
          }),
        };
      }
      if (item.kind === 'gate_action') {
        return {
          title: t('videoGeneration.agentSession.gate.actionTitle', {
            defaultValue: '素材就绪，开始生成？',
          }),
          body: t('videoGeneration.agentSession.gate.actionBody', {
            defaultValue: '将按参考视频的动作生成成片。确认即开始。',
          }),
        };
      }
      if (item.kind === 'film_ready') {
        return {
          title: t('videoGeneration.studio.filmReady', { defaultValue: '成片已就绪' }),
          body: t('videoGeneration.agentSession.filmHint', {
            defaultValue: '完整播放在左侧主列。',
          }),
        };
      }
      if (item.kind === 'failure') {
        const raw = item.error?.trim();
        const detailParts = [failure?.errorCode, failure?.providerMessage, raw].filter(
          (part, index, all) => Boolean(part) && all.indexOf(part) === index
        );
        return {
          title: failure?.title ?? t('videoGeneration.workspace.failure.unknownTitle'),
          body: [
            failure?.hint,
            failure?.providerMessage && failure.providerMessage !== failure.hint
              ? failure.providerMessage
              : '',
            relatedModel
              ? t('videoGeneration.workspace.progress.currentModel', { model: relatedModel })
              : '',
          ]
            .filter(Boolean)
            .join('\n'),
          detail: detailParts.length > 0 ? detailParts.join('\n\n') : undefined,
          issueKind: failure?.kind,
        };
      }
      if (item.kind === 'cancelled') {
        const terminal = item.stage === 'interrupted' ? 'interrupted' : 'cancelled';
        return {
          title: statusLabel(terminal, t),
          body: t('videoGeneration.agentSession.cancelledHint', {
            defaultValue: '可从断点继续，已成功的片段不会重复扣费。',
          }),
        };
      }
      const label = stageLabel(item.stage, t);
      const parts: string[] = [];
      if (item.live) {
        parts.push(t('videoGeneration.agentSession.working', { defaultValue: '正在推进…' }));
      }
      if (item.stage === 'video_poll' && typeof item.pollWaitSecs === 'number') {
        parts.push(
          t('videoGeneration.workspace.progress.pollWait', {
            secs: item.pollWaitSecs,
            defaultValue: '已等待 {{secs}} 秒',
          })
        );
      }
      if (item.live && item.at) {
        const start = Date.parse(item.at);
        if (!Number.isNaN(start)) {
          parts.push(formatElapsedClock((nowMs - start) / 1000));
        }
      }
      return { title: label, body: '', meta: parts.join(' · ') };
    },
    [failure, nowMs, relatedModel, t]
  );

  const handleSend = () => {
    if (action === 'plan') onPlan();
    else if (action === 'render') onRender();
    else if (action === 'continue') onContinue();
  };

  const handleOpenMedia = (item: StudioSessionMedia, gallery: StudioSessionMedia[]) => {
    if (item.sceneId) onFocusScene?.(item.sceneId);
    else onSelectArtifact?.(item.path);
    const list = gallery.length > 0 ? gallery : [item];
    const index = Math.max(
      0,
      list.findIndex((entry) => entry.id === item.id)
    );
    setLightbox({ items: list, index });
  };

  const startResize = useCallback(
    (event: React.MouseEvent) => {
      event.preventDefault();
      const rightEdge = columnRef.current?.getBoundingClientRect().right ?? 0;
      const shellW = columnRef.current?.parentElement?.clientWidth ?? window.innerWidth;
      const move = (e: MouseEvent) => {
        onWidthChange(clampStudioSessionWidth(rightEdge - e.clientX, shellW));
      };
      const stop = () => {
        document.body.style.cursor = '';
        document.body.style.userSelect = '';
        document.removeEventListener('mousemove', move);
        document.removeEventListener('mouseup', stop);
      };
      document.body.style.cursor = 'col-resize';
      document.body.style.userSelect = 'none';
      document.addEventListener('mousemove', move);
      document.addEventListener('mouseup', stop);
    },
    [onWidthChange]
  );

  const phaseLabel = activeSegment
    ? t(STAGE_LABEL[activeSegment.key].key, { defaultValue: STAGE_LABEL[activeSegment.key].fallback })
    : t('videoGeneration.agentSession.idlePhase', { defaultValue: '待开始' });

  const status = runStatus?.status;

  if (isMobile && collapsed) {
    return (
      <div className={`${styles.column} ${styles.mobileSheet} ${styles.mobileSheetCollapsed}`}>
        <button
          type='button'
          className={styles.header}
          onClick={() => onCollapsedChange(false)}
        >
          <div className={styles.headerMeta}>
            <Robot theme='outline' size={14} fill='currentColor' />
            <span className={styles.title}>
              {t('videoGeneration.agentSession.title', { defaultValue: 'Agent 会话' })}
              {' · '}
              {phaseLabel}
            </span>
          </div>
          <Up theme='outline' size={14} fill='currentColor' />
        </button>
      </div>
    );
  }

  return (
    <aside
      ref={columnRef}
      className={`${styles.column} ${isMobile ? styles.mobileSheet : ''}`}
      style={isMobile ? undefined : { width }}
      aria-label={t('videoGeneration.agentSession.title', { defaultValue: 'Agent 会话' })}
    >
      {!isMobile && !collapsed ? (
        <button
          type='button'
          className={styles.resizeHandle}
          onMouseDown={startResize}
          aria-label={t('videoGeneration.agentSession.resize', { defaultValue: '调整会话宽度' })}
        />
      ) : null}
      <div className={styles.header}>
        <div className={styles.headerMeta}>
          <h2 className={styles.title}>
            {t('videoGeneration.agentSession.title', { defaultValue: 'Agent 会话' })}
          </h2>
          <Tag size='small' color='arcoblue'>
            {phaseLabel}
          </Tag>
          {status ? (
            <Tag size='small' color={statusTagColor(status)}>
              {statusLabel(status, t)}
            </Tag>
          ) : null}
          {liveBusy ? <Spin size={12} /> : null}
          {liveCredits > 0 ? (
            <span
              data-testid='session-video-credits-live'
              className='text-11px tabular-nums text-[var(--color-text-3)]'
            >
              {t('videoGeneration.studio.creditsConsumed', {
                credits: liveCredits,
                defaultValue: '消耗 {{credits}} 积分',
              })}
            </span>
          ) : null}
        </div>
        <Button
          type='text'
          size='mini'
          className='!px-4px'
          onClick={() => onCollapsedChange(true)}
          aria-label={t('videoGeneration.agentSession.collapse', { defaultValue: '收起会话' })}
        >
          {isMobile ? (
            <Down theme='outline' size={14} fill='currentColor' />
          ) : (
            <CloseSmall theme='outline' size={14} fill='currentColor' />
          )}
        </Button>
      </div>
      {liveBusy || progress > 0 ? (
        <div className={styles.progressTrack} aria-hidden>
          <div
            className={styles.progressFill}
            style={{ width: `${liveBusy && progress < 3 ? 3 : progress}%` }}
          />
        </div>
      ) : null}
      {staleHint ? <div className={styles.staleHint}>{staleHint}</div> : null}
      <div ref={scrollerRef} className={styles.messages}>
        {messages.map((item) => {
          const copy = copyFor(item);
          return (
            <StudioSessionMessageView
              key={item.id}
              sessionId={sessionId}
              item={item}
              title={copy.title}
              body={copy.body}
              meta={copy.meta}
              detail={copy.detail}
              issueKind={copy.issueKind}
              onOpenMedia={handleOpenMedia}
            />
          );
        })}
      </div>
      <StudioSessionComposer
        action={action}
        onSend={handleSend}
        onStop={onCancel}
        stopping={cancelling}
        busyKind={
          planning || runStatus?.status === 'planning'
            ? 'planning'
            : rendering || liveBusy
              ? 'rendering'
              : null
        }
        assetsBlocked={
          isAction && !actionAssetsReady && !hasFinalVideo && !liveBusy && !isFailed
        }
      />
      {lightbox ? (
        <StudioMediaLightbox
          sessionId={sessionId}
          items={lightbox.items}
          index={lightbox.index}
          onIndexChange={(next) => setLightbox((prev) => (prev ? { ...prev, index: next } : prev))}
          onClose={() => setLightbox(null)}
        />
      ) : null}
    </aside>
  );
};

export default StudioAgentSession;
