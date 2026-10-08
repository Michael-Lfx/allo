import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Spin } from '@arco-design/web-react';
import { Left, LoadingFour, Right, VideoOne } from '@icon-park/react';
import { approveShot, cancelSession, getArtifact, listShotPackets } from '../api';
import { useArcoMessage } from '@renderer/utils/ui/useArcoMessage';
import type { ShotPacket, ShotRunState } from '../types';
import ShotPacketInspector from './ShotPacketInspector';
import { seekMediaElementToFirstFrame } from '../mediaFirstFrame';
import { useArtifactMediaUrl } from '../useArtifactMediaUrl';
import {
  buildStoryboardScenesFromStoryboards,
  findShotCreditPaths,
  findStoryboardPaths,
  mergeStoryboardsWithoutGrowth,
  parseStoryboard,
  shotLocationFromPath,
  storyboardRefreshSignature,
  type StoryboardScene,
  type StoryboardShot,
} from '../artifactPresentation';
import {
  creditsByShotFromSessionEvents,
  parseShotCreditsFile,
  resolveShotCreditsConsumed,
  shotCreditKey,
} from '../shotCredits';
import {
  activeVideoGenerationTarget,
  resolveStoryboardVideoStatus,
  storyboardFilmstripBadge,
  type StoryboardVideoSlotStatus,
} from '../storyboardVideoStatus';
import type { ArtifactNode } from '../types';
import { patchRunStatus, useRunStatusFull } from '../useRunStatusFeed';
import styles from '../index.module.css';

function packetKey(sceneRoot: string | undefined, shotIdx: number | undefined): string {
  return `${(sceneRoot ?? '').replace(/\\/g, '/')}:${shotIdx ?? 0}`;
}

function packetForScene(packets: ShotPacket[], scene: StoryboardScene): ShotPacket | undefined {
  const root = (scene.sceneRoot ?? '').replace(/\\/g, '/');
  const idx = scene.shotIndex ?? 0;
  return packets.find((packet) => packet.scene_root.replace(/\\/g, '/') === root && packet.shot_idx === idx);
}

function shotBadgeClass(state: ShotRunState | 'ready' | undefined): string {
  switch (state) {
    case 'awaiting_review':
      return styles.shotBadgeReview;
    case 'generating':
      return styles.shotBadgeGenerating;
    case 'ready':
      return styles.shotBadgeReady;
    case 'script_stale':
      return styles.shotBadgeScript;
    case 'continuity_stale':
      return styles.shotBadgeContinuity;
    case 'failed':
      return styles.shotBadgeFailed;
    default:
      return styles.shotBadgeIdle;
  }
}

interface StoryboardBoardProps {
  sessionId: string;
  artifacts: ArtifactNode[];
  /** Select this filmstrip card when the agent session focuses a shot. */
  focusSceneId?: string | null;
  /** Keep workspace focus in sync when the user picks a filmstrip card. */
  onFocusScene?: (sceneId: string) => void;
  /** Published clip count for the panel header. */
  onShotCount?: (count: number) => void;
  imageModel?: string | null;
  videoModel?: string | null;
}

interface SceneMediaProps {
  sessionId: string;
  path?: string;
  video?: boolean;
  compact?: boolean;
  alt: string;
  videoStatus?: StoryboardVideoSlotStatus;
}

const VideoStatusPlaceholder: React.FC<{
  compact?: boolean;
  status: Exclude<StoryboardVideoSlotStatus, 'ready'>;
}> = ({ compact, status }) => {
  const { t } = useTranslation();
  const generating = status === 'generating';
  return (
    <div
      className={[
        styles.videoStatusPlaceholder,
        generating ? styles.videoStatusGenerating : styles.videoStatusPending,
        compact ? styles.videoStatusCompact : '',
      ]
        .filter(Boolean)
        .join(' ')}
      role='status'
      aria-live={generating ? 'polite' : undefined}
    >
      {generating ? (
        <LoadingFour theme='outline' size={compact ? 16 : 28} className={styles.videoStatusIcon} />
      ) : (
        <VideoOne theme='outline' size={compact ? 16 : 28} className={styles.videoStatusIcon} />
      )}
      <div className={styles.videoStatusLabel}>
        {generating
          ? t('videoGeneration.studio.storyboard.videoGenerating', {
              defaultValue: 'Video generating',
            })
          : t('videoGeneration.studio.storyboard.videoPending', {
              defaultValue: 'Video pending',
            })}
      </div>
      {compact ? null : (
        <div className={styles.videoStatusHint}>
          {generating
            ? t('videoGeneration.studio.storyboard.videoGeneratingHint', {
                defaultValue: 'This shot is rendering now…',
              })
            : t('videoGeneration.studio.storyboard.videoPendingHint', {
                defaultValue: 'Click Generate film or Continue at the bottom right — the clip appears here',
              })}
        </div>
      )}
    </div>
  );
};

const SceneMedia: React.FC<SceneMediaProps> = ({
  sessionId,
  path,
  video,
  compact,
  alt,
  videoStatus = 'pending',
}) => {
  const { url, failed, reload } = useArtifactMediaUrl(sessionId, path ?? null);

  // Ready clip — show the video player / filmstrip preview.
  if (video && path) {
    if (failed) {
      return <VideoOne theme='outline' size={compact ? 20 : 34} className='opacity-35' />;
    }
    if (!url) return <Spin size={compact ? 12 : 18} />;
    return (
      <video
        key={path}
        src={url}
        controls={!compact}
        muted={compact}
        playsInline
        preload={compact ? 'metadata' : 'auto'}
        className={compact ? 'h-full w-full object-contain' : styles.storyShot}
        onError={() => reload()}
        onLoadedMetadata={(event) => {
          if (compact) seekMediaElementToFirstFrame(event.currentTarget);
        }}
      />
    );
  }

  // Still-frame mode (filmstrip thumbs prefer video_last_frame.png so
  // compact cells never download a whole clip just to show a thumbnail).
  const status: Exclude<StoryboardVideoSlotStatus, 'ready'> =
    videoStatus === 'generating' ? 'generating' : 'pending';

  if (videoStatus === 'ready') {
    if (!url) return <Spin size={compact ? 12 : 18} />;
    if (failed) {
      return <VideoOne theme='outline' size={compact ? 20 : 34} className='opacity-35' />;
    }
    return (
      <img
        key={path}
        src={url}
        alt={alt}
        className={compact ? 'h-full w-full object-cover' : styles.storyShot}
        onError={() => reload()}
      />
    );
  }

  // Pending / generating — optional still underneath a high-contrast status chip.
  if (path && url && !failed) {
    return (
      <div className={styles.videoStatusMediaWrap}>
        <img
          src={url}
          alt={alt}
          className={compact ? 'h-full w-full object-cover opacity-50' : `${styles.storyShot} opacity-50`}
          onError={() => reload()}
        />
        <div className={styles.videoStatusOverlay}>
          <VideoStatusPlaceholder compact={compact} status={status} />
        </div>
      </div>
    );
  }

  return <VideoStatusPlaceholder compact={compact} status={status} />;
};

const StoryboardBoard: React.FC<StoryboardBoardProps> = ({
  sessionId,
  artifacts,
  focusSceneId,
  onFocusScene,
  onShotCount,
  imageModel,
  videoModel,
}) => {
  const { t } = useTranslation();
  const [message, messageHolder] = useArcoMessage();
  const runStatus = useRunStatusFull();
  const storyboardPaths = useMemo(() => findStoryboardPaths(artifacts), [artifacts]);
  const storyboardPathKey = storyboardPaths.join('|');
  const storyboardRefreshKey = useMemo(
    () => storyboardRefreshSignature(artifacts),
    [artifacts]
  );
  const [storyboardEntries, setStoryboardEntries] = useState<
    Array<{ path: string; shots: StoryboardShot[] }>
  >([]);
  const [activeSceneId, setActiveSceneId] = useState<string>();
  const [sidecarCredits, setSidecarCredits] = useState<Map<string, number>>(() => new Map());
  const [packets, setPackets] = useState<ShotPacket[]>([]);
  const [inspectorDirty, setInspectorDirty] = useState(false);
  const [reviewBusy, setReviewBusy] = useState(false);

  const generatingTarget = useMemo(
    () => activeVideoGenerationTarget(runStatus),
    [runStatus]
  );
  const planning = runStatus?.status === 'planning';
  const rendering =
    runStatus?.status === 'rendering' || runStatus?.status === 'awaiting_review';
  const awaitingReview = runStatus?.status === 'awaiting_review';
  const pendingReview = awaitingReview ? runStatus?.pending_review ?? null : null;

  // Storyboard *rows* come only from storyboard.json. Video start writes
  // shots/N/shot_description.json and the artifact poll used to refetch the
  // board in the same effect — that replaced a planned N-shot strip with N+1.
  useEffect(() => {
    if (!storyboardPathKey) return;
    let cancelled = false;
    void Promise.all(
      storyboardPaths.map(async (path) => {
        try {
          const content = await getArtifact(sessionId, path);
          return { path, shots: parseStoryboard(content.text) };
        } catch {
          return { path, shots: [] as StoryboardShot[] };
        }
      })
    ).then((boards) => {
      if (cancelled) return;
      setStoryboardEntries((previous) =>
        mergeStoryboardsWithoutGrowth(previous, boards, !rendering)
      );
    });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- paths + packed content/dirs
  }, [sessionId, storyboardPathKey, storyboardRefreshKey, rendering]);

  const scenes = useMemo(
    () => buildStoryboardScenesFromStoryboards(artifacts, storyboardEntries),
    [artifacts, storyboardEntries]
  );
  const packetRefreshKey = useMemo(
    () => scenes.map((scene) => `${scene.id}:${scene.videoPath ?? ''}`).join('|'),
    [scenes]
  );

  useEffect(() => {
    if (!sessionId) return;
    let cancelled = false;
    void listShotPackets(sessionId)
      .then((rows) => {
        if (!cancelled) setPackets(rows);
      })
      .catch(() => {
        if (!cancelled) setPackets([]);
      });
    return () => {
      cancelled = true;
    };
  }, [sessionId, runStatus?.status, runStatus?.stage, runStatus?.updated_at, scenes.length, packetRefreshKey]);

  useEffect(() => {
    onShotCount?.(scenes.length);
  }, [onShotCount, scenes.length]);

  // Snap once when the agent focuses a new shot. Re-applying on every
  // `scenes` rebuild (artifact poll) stole the card the user just clicked.
  const syncedFocusSceneIdRef = useRef<string | null>(null);
  useEffect(() => {
    if (!focusSceneId) return;
    if (!scenes.some((scene) => scene.id === focusSceneId)) return;
    if (syncedFocusSceneIdRef.current === focusSceneId) return;
    syncedFocusSceneIdRef.current = focusSceneId;
    setActiveSceneId(focusSceneId);
  }, [focusSceneId, scenes]);

  const pendingKey = pendingReview
    ? packetKey(pendingReview.scene_root, pendingReview.shot_idx)
    : '';
  const pendingSyncedRef = useRef('');
  useEffect(() => {
    if (!pendingKey || scenes.length === 0) return;
    if (pendingSyncedRef.current === pendingKey) return;
    const match = scenes.find(
      (scene) => packetKey(scene.sceneRoot, scene.shotIndex) === pendingKey
    );
    if (!match) return;
    pendingSyncedRef.current = pendingKey;
    syncedFocusSceneIdRef.current = match.id;
    setActiveSceneId(match.id);
    onFocusScene?.(match.id);
  }, [pendingKey, scenes, onFocusScene]);

  const selectScene = useCallback(
    (sceneId: string) => {
      syncedFocusSceneIdRef.current = sceneId;
      setActiveSceneId(sceneId);
      onFocusScene?.(sceneId);
    },
    [onFocusScene]
  );

  const creditPathKey = useMemo(() => findShotCreditPaths(artifacts).join('|'), [artifacts]);

  useEffect(() => {
    const paths = creditPathKey ? creditPathKey.split('|') : [];
    if (paths.length === 0) {
      setSidecarCredits(new Map());
      return;
    }
    let cancelled = false;
    void Promise.all(
      paths.map(async (path) => {
        try {
          const content = await getArtifact(sessionId, path);
          return { path, credits: parseShotCreditsFile(content.text) };
        } catch {
          return { path, credits: 0 };
        }
      })
    ).then((rows) => {
      if (cancelled) return;
      const next = new Map<string, number>();
      for (const row of rows) {
        if (row.credits <= 0) continue;
        const location = shotLocationFromPath(row.path);
        if (!location) continue;
        const key = shotCreditKey(location.sceneRoot, location.shotIndex);
        if (!key) continue;
        next.set(key, Math.max(next.get(key) ?? 0, row.credits));
      }
      setSidecarCredits(next);
    });
    return () => {
      cancelled = true;
    };
  }, [creditPathKey, sessionId]);

  const eventCredits = useMemo(
    () => creditsByShotFromSessionEvents(runStatus?.events),
    [runStatus?.events]
  );

  const creditsForScene = useCallback(
    (scene: StoryboardScene): number =>
      resolveShotCreditsConsumed({
        sceneRoot: scene.sceneRoot,
        shotIndex: scene.shotIndex,
        hasVideo: Boolean(scene.videoPath),
        eventCredits,
        sidecarCredits,
      }),
    [eventCredits, sidecarCredits]
  );

  const activeScene =
    scenes.find((scene) => scene.id === activeSceneId) ??
    scenes[0];

  const videoStatusFor = useCallback(
    (scene: StoryboardScene): StoryboardVideoSlotStatus =>
      resolveStoryboardVideoStatus({
        hasVideo: Boolean(scene.videoPath),
        shotIndex: scene.shotIndex,
        sceneRoot: scene.sceneRoot,
        rendering,
        target: generatingTarget,
      }),
    [generatingTarget, rendering]
  );

  const filmstripRef = useRef<HTMLDivElement>(null);
  const [canScrollLeft, setCanScrollLeft] = useState(false);
  const [canScrollRight, setCanScrollRight] = useState(false);

  const updateFilmstripOverflow = useCallback(() => {
    const el = filmstripRef.current;
    if (!el) {
      setCanScrollLeft(false);
      setCanScrollRight(false);
      return;
    }
    const maxScroll = el.scrollWidth - el.clientWidth;
    const epsilon = 2;
    setCanScrollLeft(el.scrollLeft > epsilon);
    setCanScrollRight(maxScroll - el.scrollLeft > epsilon);
  }, []);

  const scrollFilmstrip = useCallback((direction: -1 | 1) => {
    const el = filmstripRef.current;
    if (!el) return;
    const distance = Math.max(el.clientWidth * 0.72, 176);
    el.scrollBy({ left: direction * distance, behavior: 'smooth' });
  }, []);

  useLayoutEffect(() => {
    const el = filmstripRef.current;
    if (!el) return;
    updateFilmstripOverflow();
    const observer = new ResizeObserver(updateFilmstripOverflow);
    observer.observe(el);
    for (const child of Array.from(el.children)) {
      observer.observe(child);
    }
    el.addEventListener('scroll', updateFilmstripOverflow, { passive: true });
    window.addEventListener('resize', updateFilmstripOverflow);
    return () => {
      observer.disconnect();
      el.removeEventListener('scroll', updateFilmstripOverflow);
      window.removeEventListener('resize', updateFilmstripOverflow);
    };
  }, [scenes.length, updateFilmstripOverflow]);

  useEffect(() => {
    const strip = filmstripRef.current;
    if (!strip || !activeScene) return;
    const card = strip.querySelector<HTMLElement>(
      `[data-scene-id="${CSS.escape(activeScene.id)}"]`
    );
    if (!card) return;
    const cardLeft = card.offsetLeft;
    const cardRight = cardLeft + card.offsetWidth;
    const viewLeft = strip.scrollLeft;
    const viewRight = viewLeft + strip.clientWidth;
    if (cardLeft < viewLeft) {
      strip.scrollTo({ left: cardLeft, behavior: 'smooth' });
    } else if (cardRight > viewRight) {
      strip.scrollTo({ left: cardRight - strip.clientWidth, behavior: 'smooth' });
    }
  }, [activeScene?.id]);

  const showFilmstripNav = scenes.length > 1;

  if (!activeScene) {
    return (
      <div className={styles.storyboardPreparing}>
        <VideoOne theme='outline' size={28} className='text-[var(--color-text-3)]' />
        <div className='text-13px font-600 text-[var(--color-text-1)]'>
          {t('videoGeneration.studio.storyboard.preparing', {
            defaultValue: '正在整理分镜画面',
          })}
        </div>
        <div className='max-w-400px text-12px text-[var(--color-text-3)]'>
          {t('videoGeneration.studio.storyboard.preparingHint', {
            defaultValue: '规划完成后，镜头会按故事顺序出现在这里。',
          })}
        </div>
      </div>
    );
  }

  const activeVideoStatus = videoStatusFor(activeScene);
  const mainPath =
    activeScene.videoPath ??
    (activeVideoStatus === 'ready' ? undefined : activeScene.imagePath);
  const mainIsVideo = Boolean(activeScene.videoPath);
  const sceneNumber = activeScene.index + 1;
  const activePacket = packetForScene(packets, activeScene);
  const pendingScene = pendingReview
    ? scenes.find(
        (scene) =>
          packetKey(scene.sceneRoot, scene.shotIndex) ===
          packetKey(pendingReview.scene_root, pendingReview.shot_idx)
      )
    : undefined;
  const pendingNumber = pendingScene ? pendingScene.index + 1 : pendingReview?.shot_idx != null
    ? pendingReview.shot_idx + 1
    : 0;
  const reviewLocked =
    awaitingReview &&
    Boolean(pendingReview) &&
    packetKey(activeScene.sceneRoot, activeScene.shotIndex) !==
      packetKey(pendingReview?.scene_root, pendingReview?.shot_idx);
  const shotGenerating =
    videoStatusFor(activeScene) === 'generating' || activePacket?.run_state === 'generating';

  const handleApprove = async (switchToContinuous = false) => {
    if (!pendingReview || inspectorDirty) return;
    setReviewBusy(true);
    try {
      await approveShot(
        sessionId,
        pendingReview.scene_root,
        pendingReview.shot_idx,
        switchToContinuous
      );
      if (switchToContinuous) {
        patchRunStatus({ render_mode: 'continuous' });
      }
    } catch (err) {
      message.error(err instanceof Error ? err.message : String(err));
    } finally {
      setReviewBusy(false);
    }
  };

  const handlePausePipeline = async () => {
    setReviewBusy(true);
    try {
      await cancelSession(sessionId);
    } catch (err) {
      message.error(err instanceof Error ? err.message : String(err));
    } finally {
      setReviewBusy(false);
    }
  };

  return (
    <>
      {messageHolder}
    <div className={styles.storyboardLayout}>
      <div className={`${styles.storyStage} ${pendingReview ? styles.storyStageReview : ''}`}>
        <ShotPacketInspector
          sessionId={sessionId}
          scene={activeScene}
          artifacts={artifacts}
          generating={shotGenerating}
          planning={planning}
          reviewLocked={reviewLocked}
          shotNumber={sceneNumber}
          shotTotal={scenes.length}
          imageModel={imageModel}
          videoModel={videoModel}
          onUnsavedChange={setInspectorDirty}
          videoPath={activeScene.videoPath}
          posterPath={mainIsVideo ? activeScene.imagePath : mainPath}
          videoStatus={activeVideoStatus}
          preview={
            <>
              {activeScene.beatCount != null && activeScene.beatCount > 1 ? (
                <span className={styles.shotGraphMediaChip}>
                  {t('videoGeneration.studio.storyboard.packedBeatsShort', {
                    count: activeScene.beatCount,
                    defaultValue: '{{count}} 切',
                  })}
                </span>
              ) : null}
              {activePacket?.run_state === 'script_stale' || activePacket?.run_state === 'continuity_stale' ? (
                <span className={styles.shotStaleBanner}>
                  {activePacket.run_state === 'script_stale'
                    ? t('videoGeneration.studio.storyboard.scriptStaleHint', {
                        defaultValue: '脚本已改，画面仍是旧版',
                      })
                    : t('videoGeneration.studio.storyboard.continuityStaleHint', {
                        defaultValue: '上一镜已换 take，连续性参考已过期',
                      })}
                </span>
              ) : null}
            </>
          }
        />
        {pendingReview ? (
          <div className={styles.shotReviewBar} data-testid='shot-review-bar'>
            <div className={styles.shotReviewCopy}>
              {t('videoGeneration.studio.storyboard.reviewBar', {
                number: pendingNumber,
                total: scenes.length,
                duration: pendingReview.duration_secs || '—',
                images: pendingReview.image_ref_count,
                audio: pendingReview.audio_ref_count,
                defaultValue:
                  '镜头 {{number}} / {{total}} · 待过审 · 约 {{duration}}s · 参考图 {{images}} · 音色 {{audio}}',
              })}
              {inspectorDirty
                ? ` · ${t('videoGeneration.studio.storyboard.saveBeforeApprove', {
                    defaultValue: '请先保存脚本',
                  })}`
                : ''}
            </div>
            <div className={styles.shotReviewActions}>
              <button
                type='button'
                className={styles.shotReviewPrimary}
                disabled={inspectorDirty || reviewBusy}
                data-testid='shot-review-approve'
                onClick={() => void handleApprove(false)}
              >
                {t('videoGeneration.studio.storyboard.approveShot', { defaultValue: '确认生成' })}
              </button>
              <button
                type='button'
                className={styles.shotReviewSecondary}
                disabled={reviewBusy}
                onClick={() => void handleApprove(true)}
              >
                {t('videoGeneration.studio.storyboard.switchContinuous', {
                  defaultValue: '改为连续出片',
                })}
              </button>
              <button
                type='button'
                className={styles.shotReviewSecondary}
                disabled={reviewBusy}
                onClick={() => void handlePausePipeline()}
              >
                {t('videoGeneration.studio.storyboard.pausePipeline', { defaultValue: '暂停流水线' })}
              </button>
            </div>
          </div>
        ) : null}
      </div>

      {showFilmstripNav ? (
        <button
          type='button'
          className={`${styles.filmstripNav} ${styles.filmstripNavPrev}`}
          disabled={!canScrollLeft}
          aria-label={t('videoGeneration.studio.storyboard.filmstripPrev', {
            defaultValue: '向左查看镜头',
          })}
          title={t('videoGeneration.studio.storyboard.filmstripPrev', {
            defaultValue: '向左查看镜头',
          })}
          onClick={() => scrollFilmstrip(-1)}
        >
          <Left theme='outline' size={12} />
        </button>
      ) : (
        <span className={`${styles.filmstripGutter} ${styles.filmstripGutterPrev}`} aria-hidden />
      )}
      <div
        ref={filmstripRef}
        className={styles.filmstrip}
        aria-label={t('videoGeneration.studio.storyboard.filmstrip', {
          defaultValue: '分镜胶片',
        })}
        tabIndex={showFilmstripNav ? 0 : undefined}
        onKeyDown={(event) => {
          if (event.key === 'ArrowLeft') {
            event.preventDefault();
            scrollFilmstrip(-1);
          } else if (event.key === 'ArrowRight') {
            event.preventDefault();
            scrollFilmstrip(1);
          }
        }}
      >
          {scenes.map((scene) => {
            const number = scene.index + 1;
            const active = scene.id === activeScene.id;
            const status = videoStatusFor(scene);
            const shotCredits = creditsForScene(scene);
            const packet = packetForScene(packets, scene);
            const runState = packet?.run_state;
            const isReviewing =
              pendingReview &&
              packetKey(scene.sceneRoot, scene.shotIndex) ===
                packetKey(pendingReview.scene_root, pendingReview.shot_idx);
            const thumbPath = scene.imagePath ?? scene.videoPath;
            const badge = storyboardFilmstripBadge({ runState, videoStatus: status });
            const badgeLabel =
              badge === 'awaiting_review'
                ? t('videoGeneration.studio.storyboard.badgeReview', { defaultValue: '待过审' })
                : badge === 'generating'
                  ? t('videoGeneration.studio.storyboard.badgeGenerating', { defaultValue: '生成中' })
                  : badge === 'ready'
                    ? t('videoGeneration.studio.storyboard.badgeReady', { defaultValue: '已出片' })
                    : badge === 'script_stale'
                      ? t('videoGeneration.studio.storyboard.badgeScriptStale', { defaultValue: '脚本已改' })
                      : badge === 'continuity_stale'
                        ? t('videoGeneration.studio.storyboard.badgeContinuity', { defaultValue: '连续性过期' })
                        : badge === 'failed'
                          ? t('videoGeneration.studio.storyboard.badgeFailed', { defaultValue: '失败' })
                          : null;
            return (
              <div key={scene.id} className={styles.shotCardStack} data-scene-id={scene.id}>
              <button
                type='button'
                className={`${styles.shotCard} ${active ? styles.shotCardActive : ''} ${
                  status === 'generating' || runState === 'generating' ? styles.shotCardGenerating : ''
                } ${isReviewing ? styles.shotCardReview : ''}`}
                aria-pressed={active}
                onClick={() => selectScene(scene.id)}
              >
              <span className={styles.shotThumb}>
                <SceneMedia
                  sessionId={sessionId}
                  path={thumbPath}
                  video={!scene.imagePath && Boolean(scene.videoPath)}
                  compact
                  alt={t('videoGeneration.studio.storyboard.shotAlt', {
                    number,
                    defaultValue: '镜头 {{number}}',
                  })}
                  videoStatus={status}
                />
                <span className='absolute bottom-6px left-6px z-1 rd-full bg-black/70 px-6px py-2px text-10px font-700 text-white'>
                  {String(number).padStart(2, '0')}
                </span>
                {packet?.current_take ? (
                  <span className={styles.shotTakeBadge}>v{packet.current_take}</span>
                ) : null}
                {shotCredits > 0 ? (
                  <span
                    data-testid='shot-video-credits'
                    className={styles.shotCardCredits}
                    title={t('videoGeneration.studio.creditsConsumed', {
                      credits: shotCredits,
                      defaultValue: '消耗 {{credits}} 积分',
                    })}
                  >
                    {t('videoGeneration.studio.creditsConsumed', {
                      credits: shotCredits,
                      defaultValue: '消耗 {{credits}} 积分',
                    })}
                  </span>
                ) : null}
                {badgeLabel || scene.beatCount != null ? (
                  <span className={styles.shotThumbMetaBr}>
                    {scene.beatCount != null ? (
                      <span className={styles.shotPackedBadge}>
                        {t('videoGeneration.studio.storyboard.packedBeatsShort', {
                          count: scene.beatCount,
                          defaultValue: '{{count}} 切',
                        })}
                      </span>
                    ) : null}
                    {badgeLabel ? (
                      <span className={`${styles.shotStateBadge} ${shotBadgeClass(badge ?? undefined)}`}>
                        {badgeLabel}
                      </span>
                    ) : null}
                  </span>
                ) : null}
              </span>
              <span className='block truncate px-9px py-8px text-11px'>
                {scene.visualDescription ||
                  t('videoGeneration.studio.storyboard.shotNumber', {
                    number,
                    defaultValue: '镜头 {{number}}',
                  })}
              </span>
            </button>
                {scene.beats && scene.beats.length >= 2 ? (
                  <ol className={styles.shotCoverageList}>
                    {scene.beats.map((beat, beatIndex) => (
                      <li key={`${scene.id}-beat-${beatIndex}`}>
                        {t('videoGeneration.studio.storyboard.packedBeatItem', {
                          number: beatIndex + 1,
                          defaultValue: '切镜 {{number}}',
                        })}
                      </li>
                    ))}
                  </ol>
                ) : null}
              </div>
            );
          })}
        </div>
      {showFilmstripNav ? (
        <button
          type='button'
          className={`${styles.filmstripNav} ${styles.filmstripNavNext}`}
          disabled={!canScrollRight}
          aria-label={t('videoGeneration.studio.storyboard.filmstripNext', {
            defaultValue: '向右查看镜头',
          })}
          title={t('videoGeneration.studio.storyboard.filmstripNext', {
            defaultValue: '向右查看镜头',
          })}
          onClick={() => scrollFilmstrip(1)}
        >
          <Right theme='outline' size={12} />
        </button>
      ) : (
        <span className={`${styles.filmstripGutter} ${styles.filmstripGutterNext}`} aria-hidden />
      )}
    </div>
    </>
  );
};

export default StoryboardBoard;
