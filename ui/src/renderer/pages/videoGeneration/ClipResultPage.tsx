/**
 * 「即刻出片」 detail (`/video-generation/clip/:taskId`).
 *
 * The page lives inside the app shell, which clips overflow. It must fill that
 * pane and scroll itself — a 100vh page gets cut off under the titlebar.
 * The preview frame tracks the real picture ratio, and a failed clip starts a
 * new task instead of reloading the same error.
 */
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useLocation, useNavigate, useParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { RollingDuration, TimedPhrase, timerSlot } from '@renderer/components/base/Counter';
import { Button } from '@arco-design/web-react';
import {
  ArrowLeft,
  Refresh,
  Error,
  LoadingOne,
  FullScreen,
  OffScreen,
  PlayOne,
  PauseOne,
  VolumeMute,
  VolumeNotice,
  Copy,
  Check,
  FolderOpen,
} from '@icon-park/react';
import { useCloudAuth } from '@renderer/hooks/context/CloudAuthContext';
import { useCredits } from '@renderer/hooks/context/CreditsContext';
import { useArcoMessage } from '@renderer/utils/ui/useArcoMessage';
import { trackFunnelEvent, trackLowCreditBalance } from '@renderer/utils/analytics/productFunnel';
import { openInAppBilling } from '@renderer/utils/openInAppBilling';
import {
  getGenerationTask,
  createGenerationTask,
  canvasMediaUrl,
  getMediaPath,
  type GenerationTaskView,
} from '../videoCanvas/api';
import { ipcBridge } from '@/common';
import { rememberVideoGenerationTask } from './routeMemory';
import { describeClipFailure, formatClipOperationError, type ClipFailureView } from './clipFailure';
import {
  exitElementFullscreen,
  isElementFullscreen,
  requestElementFullscreen,
  subscribeFullscreenChange,
} from './clipPlayerFullscreen';
import {
  clipFollowUp,
  clipFrameAspect,
  clipProgressPercent,
  clipRegenerateBody,
  clipStepKey,
  clipStillIds,
  clipTimestampMs,
  shortClipModel,
  type ClipTaskStatus,
} from './clipResultModel';
import styles from './ClipResultPage.module.css';

type ClipStatusErrorKey = 'taskNotFound' | 'generationCanceled' | 'generationTimeout';

const POLL_INTERVAL_MS = 2000;
const MAX_POLL_ATTEMPTS = 300;
const TRANSIENT_POLL_LIMIT = 4;

type LocationState = {
  title?: string;
  prompt?: string;
  taskId?: string;
};

function frameStyle(aspect: number): React.CSSProperties {
  return { '--clip-aspect': String(aspect) } as React.CSSProperties;
}

function statusTone(status: ClipTaskStatus): 'running' | 'ok' | 'bad' | 'idle' {
  if (status === 'queued' || status === 'running') return 'running';
  if (status === 'succeeded') return 'ok';
  if (status === 'failed' || status === 'canceled') return 'bad';
  return 'idle';
}

function ClipFailurePanel({
  view,
  layout,
  onRetry,
  onBilling,
  retryLabel,
  billingLabel,
  busy,
}: {
  view: ClipFailureView;
  layout: 'page' | 'panel';
  onRetry: () => void;
  onBilling?: () => void;
  retryLabel: string;
  billingLabel: string;
  busy?: boolean;
}) {
  const billingButton = view.recovery ? (
    <Button
      shape='round'
      type='primary'
      className='flowy-icon-text-btn'
      data-testid='video-failure-open-billing'
      onClick={onBilling}
    >
      {billingLabel}
    </Button>
  ) : null;

  const retryButton = (
    <Button
      shape='round'
      type='primary'
      className='flowy-icon-text-btn'
      icon={<Refresh theme='outline' size={14} />}
      loading={busy}
      disabled={busy}
      onClick={onRetry}
    >
      {retryLabel}
    </Button>
  );

  const action = billingButton ?? retryButton;

  if (layout === 'panel') {
    return (
      <div className={styles.errorPanel} role='alert'>
        <div className={styles.errorPanelHeader}>
          <Error theme='outline' size={18} className={styles.errorBannerIcon} />
          <h2 className={styles.errorPanelTitle}>{view.title}</h2>
        </div>
        <p className={styles.errorPanelMessage}>{view.message}</p>
        <div className={styles.errorPanelActions}>{action}</div>
      </div>
    );
  }

  return (
    <div className={styles.errorState} role='alert'>
      <div className={styles.errorIcon}>
        <Error theme='outline' size={28} />
      </div>
      <h2 className={styles.errorTitle}>{view.title}</h2>
      <p className={styles.errorMessage}>{view.message}</p>
      <div className={styles.errorActions}>{action}</div>
    </div>
  );
}

function ReferenceStills({ ids, label }: { ids: string[]; label: string }) {
  const [broken, setBroken] = useState<Record<string, true>>({});
  const visible = ids.filter((id) => !broken[id]);
  if (visible.length === 0) return null;
  return (
    <section className={styles.stillsBlock} aria-label={label}>
      <h2 className={styles.stillsLabel}>{label}</h2>
      <ul className={styles.stills}>
        {visible.map((id) => (
          <li key={id}>
            <img
              src={canvasMediaUrl(id)}
              alt={label}
              draggable={false}
              onError={() => setBroken((current) => ({ ...current, [id]: true }))}
            />
          </li>
        ))}
      </ul>
    </section>
  );
}

const ClipResultPage: React.FC = () => {
  const { t, i18n } = useTranslation();
  const navigate = useNavigate();
  const location = useLocation();
  const { taskId: urlTaskId } = useParams<{ taskId: string }>();
  const { logout } = useCloudAuth();
  const { balance } = useCredits();
  const [message, messageHolder] = useArcoMessage();

  const taskId = urlTaskId || (location.state as LocationState | null)?.taskId;
  const title = (location.state as LocationState | null)?.title;
  const prompt = (location.state as LocationState | null)?.prompt;
  const titleRef = useRef(title);
  titleRef.current = title;

  const [task, setTask] = useState<GenerationTaskView | null>(null);
  const [loading, setLoading] = useState(true);
  const [rawError, setRawError] = useState<string | null>(null);
  const [statusErrorKey, setStatusErrorKey] = useState<ClipStatusErrorKey | null>(null);
  const [videoUrl, setVideoUrl] = useState<string | null>(null);
  const [isPlaying, setIsPlaying] = useState(false);
  const [isFullscreen, setIsFullscreen] = useState(false);
  const [useCssFullscreen, setUseCssFullscreen] = useState(false);
  const [muted, setMuted] = useState(false);
  const [volume, setVolume] = useState(1);
  const [currentTime, setCurrentTime] = useState(0);
  const [duration, setDuration] = useState(0);
  const [buffered, setBuffered] = useState(0);
  const [showControls, setShowControls] = useState(true);
  const [isOpeningFolder, setIsOpeningFolder] = useState(false);
  const [isRegenerating, setIsRegenerating] = useState(false);
  const [copied, setCopied] = useState<'id' | 'prompt' | null>(null);
  const [videoAspect, setVideoAspect] = useState<number | null>(null);
  const [pollEpoch, setPollEpoch] = useState(0);
  const [now, setNow] = useState(() => Date.now());

  const pollAttemptsRef = useRef(0);
  const consecutiveErrorsRef = useRef(0);
  const fetchGenRef = useRef(0);
  const requestSeqRef = useRef(0);
  const mountedRef = useRef(true);
  const seenTaskIdRef = useRef(taskId);
  const videoRef = useRef<HTMLVideoElement>(null);
  const playerRef = useRef<HTMLDivElement>(null);
  const cssFullscreenRef = useRef(false);
  const controlsTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const copiedTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const volumeRef = useRef(volume);
  volumeRef.current = volume;

  const fetchTask = useCallback(async () => {
    const gen = fetchGenRef.current;
    const seq = ++requestSeqRef.current;
    const stale = () =>
      gen !== fetchGenRef.current || seq !== requestSeqRef.current || !mountedRef.current;

    if (!taskId) {
      if (stale()) return false;
      setLoading(false);
      setStatusErrorKey('taskNotFound');
      setRawError(null);
      return true;
    }

    try {
      const result = await getGenerationTask(taskId);
      if (stale()) return false;
      consecutiveErrorsRef.current = 0;
      setLoading(false);
      setTask(result);
      setStatusErrorKey(null);
      rememberVideoGenerationTask(
        taskId,
        titleRef.current?.trim() || result.prompt?.trim().slice(0, 48)
      );

      if (result.status === 'succeeded') {
        setRawError(null);
        setVideoUrl(result.result_media_id ? canvasMediaUrl(result.result_media_id) : null);
        return true;
      }
      if (result.status === 'failed') {
        setRawError(result.error || '');
        return true;
      }
      if (result.status === 'canceled') {
        setStatusErrorKey('generationCanceled');
        setRawError(null);
        return true;
      }
      setRawError(null);
      return false;
    } catch (e: unknown) {
      if (stale()) return false;
      const err = e as { status?: number; code?: string; message?: string };
      if (
        err?.status === 401 ||
        (err?.status === 400 &&
          /session|token|credential|authentication/i.test(err?.code || ''))
      ) {
        await logout();
        navigate('/cloud-login');
        return true;
      }
      consecutiveErrorsRef.current += 1;
      const errorMessage = (e as Error).message ?? String(e);
      if (consecutiveErrorsRef.current < TRANSIENT_POLL_LIMIT) {
        console.warn('[ClipResultPage] transient task fetch error:', errorMessage);
        return false;
      }
      console.error('[ClipResultPage] failed to fetch task:', errorMessage);
      setLoading(false);
      setStatusErrorKey(null);
      setRawError(errorMessage);
      return true;
    }
  }, [taskId, logout, navigate]);

  useEffect(() => {
    let stopped = false;
    let timer: ReturnType<typeof setInterval> | null = null;
    pollAttemptsRef.current = 0;
    consecutiveErrorsRef.current = 0;

    if (seenTaskIdRef.current !== taskId) {
      seenTaskIdRef.current = taskId;
      setTask(null);
      setVideoUrl(null);
      setVideoAspect(null);
      setIsPlaying(false);
      setCurrentTime(0);
      setDuration(0);
      setBuffered(0);
      setRawError(null);
      setStatusErrorKey(null);
      setLoading(true);
    }

    const arm = async () => {
      const finished = await fetchTask();
      if (stopped || finished) return;
      timer = setInterval(() => {
        void (async () => {
          pollAttemptsRef.current += 1;
          if (pollAttemptsRef.current >= MAX_POLL_ATTEMPTS) {
            if (timer) clearInterval(timer);
            if (!stopped) {
              setStatusErrorKey('generationTimeout');
              setRawError(null);
            }
            return;
          }
          const done = await fetchTask();
          if (done && timer) clearInterval(timer);
        })();
      }, POLL_INTERVAL_MS);
    };

    void arm();
    return () => {
      stopped = true;
      fetchGenRef.current += 1;
      if (timer) clearInterval(timer);
    };
  }, [fetchTask, pollEpoch, taskId]);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      if (controlsTimerRef.current) clearTimeout(controlsTimerRef.current);
      if (copiedTimerRef.current) clearTimeout(copiedTimerRef.current);
    };
  }, []);

  const handlePlayPause = useCallback(() => {
    const video = videoRef.current;
    if (!video) return;
    if (video.paused) void video.play();
    else video.pause();
  }, []);

  const handleFullscreen = useCallback(() => {
    const player = playerRef.current;
    if (!player) return;
    if (isElementFullscreen(player) || cssFullscreenRef.current) {
      void exitElementFullscreen();
      cssFullscreenRef.current = false;
      setUseCssFullscreen(false);
      setIsFullscreen(false);
      return;
    }
    void requestElementFullscreen(player).then((native) => {
      cssFullscreenRef.current = !native;
      setUseCssFullscreen(!native);
      setIsFullscreen(true);
    });
  }, []);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const target = event.target;
      if (
        target instanceof HTMLElement &&
        (target.isContentEditable ||
          target.closest('input, textarea, select, button, a, [role="slider"]'))
      ) {
        return;
      }
      const video = videoRef.current;
      if (!video) return;
      if (event.metaKey || event.ctrlKey || event.altKey) return;

      switch (event.key) {
        case ' ':
        case 'k':
          event.preventDefault();
          handlePlayPause();
          break;
        case 'ArrowLeft':
          event.preventDefault();
          video.currentTime = Math.max(0, video.currentTime - 5);
          break;
        case 'ArrowRight':
          event.preventDefault();
          video.currentTime = Math.min(video.duration || 0, video.currentTime + 5);
          break;
        case 'ArrowUp':
          event.preventDefault();
          setVolume(Math.min(1, Number((volumeRef.current + 0.1).toFixed(2))));
          setMuted(false);
          break;
        case 'ArrowDown': {
          event.preventDefault();
          const next = Math.max(0, Number((volumeRef.current - 0.1).toFixed(2)));
          setVolume(next);
          if (next === 0) setMuted(true);
          break;
        }
        case 'm':
          event.preventDefault();
          setMuted((value) => !value);
          break;
        case 'f':
          event.preventDefault();
          handleFullscreen();
          break;
        default:
          break;
      }
    };
    document.addEventListener('keydown', onKeyDown);
    return () => document.removeEventListener('keydown', onKeyDown);
  }, [handleFullscreen, handlePlayPause]);

  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    video.volume = volume;
    video.muted = muted;
  }, [volume, muted, videoUrl]);

  const handleBack = useCallback(() => {
    navigate('/video-generation?mode=generate');
  }, [navigate]);

  const failureView = useMemo((): ClipFailureView | null => {
    if (statusErrorKey === 'taskNotFound') {
      return {
        credits: false,
        title: t('videoGeneration.clip.errorTitle'),
        message: t('videoGeneration.clip.taskNotFound'),
        recovery: null,
      };
    }
    if (statusErrorKey === 'generationCanceled') {
      return {
        credits: false,
        title: t('videoGeneration.clip.errorTitle'),
        message: t('videoGeneration.clip.generationCanceled'),
        recovery: null,
      };
    }
    if (statusErrorKey === 'generationTimeout') {
      return {
        credits: false,
        title: t('videoGeneration.clip.errorTitle'),
        message: t('videoGeneration.clip.generationTimeout'),
        recovery: null,
      };
    }
    if (rawError == null) return null;
    return describeClipFailure(rawError, t);
  }, [rawError, statusErrorKey, t]);

  const creditsFailKeyRef = useRef<string | null>(null);
  useEffect(() => {
    if (!failureView?.credits) return;
    const key = `${taskId ?? 'unknown'}:credits`;
    if (creditsFailKeyRef.current === key) return;
    creditsFailKeyRef.current = key;
    trackLowCreditBalance({ source: 'video_failure_card', balance });
  }, [balance, failureView?.credits, taskId]);

  const handleOpenBilling = useCallback(() => {
    if (!failureView?.recovery) return;
    trackFunnelEvent('prerequisite_resolved', {
      kind: failureView.recovery.source,
      source: 'error_recovery',
      feature: 'video_generation',
    });
    openInAppBilling(navigate, {
      source: 'video_failure_card',
      balance,
    });
  }, [balance, failureView, navigate]);

  const markCopied = useCallback((kind: 'id' | 'prompt') => {
    setCopied(kind);
    if (copiedTimerRef.current) clearTimeout(copiedTimerRef.current);
    copiedTimerRef.current = setTimeout(() => setCopied(null), 1600);
  }, []);

  const copyText = useCallback(
    async (value: string, kind: 'id' | 'prompt') => {
      try {
        await navigator.clipboard.writeText(value);
        markCopied(kind);
      } catch (err) {
        message.error(
          t('videoGeneration.clip.copyFailed', {
            defaultValue: err instanceof globalThis.Error ? err.message : String(err),
          })
        );
      }
    },
    [markCopied, message, t]
  );

  const regenerate = useCallback(async () => {
    const body = clipRegenerateBody({
      mode: task?.mode,
      prompt: task?.prompt || prompt,
      model: task?.model,
      aspect_ratio: task?.aspect_ratio,
      resolution: task?.resolution,
      duration_secs: task?.duration_secs,
      reference_media_ids: task?.reference_media_ids,
      audio_media_ids: task?.audio_media_ids,
      reference_video_media_id: task?.reference_video_media_id,
      first_frame_media_id: task?.first_frame_media_id,
      last_frame_media_id: task?.last_frame_media_id,
    });
    if (!body) {
      message.error(t('videoGeneration.clip.regenerateNoPrompt'));
      return;
    }
    setIsRegenerating(true);
    try {
      const next = await createGenerationTask(body);
      navigate(`/video-generation/clip/${encodeURIComponent(next.task_id)}`, {
        replace: true,
        state: {
          title: title || body.prompt.slice(0, 48),
          prompt: body.prompt,
          taskId: next.task_id,
        },
      });
    } catch (err) {
      const failure = formatClipOperationError(err, t, {
        key: 'videoGeneration.clip.regenerateFailedPrefix',
        defaultValue: '重新生成失败',
      });
      message.error(failure.message);
      if (failure.credits) {
        trackLowCreditBalance({ source: 'video_launch', balance });
      }
    } finally {
      setIsRegenerating(false);
    }
  }, [task, prompt, title, navigate, message, t, balance]);

  const handleRetry = useCallback(async () => {
    const followUp = clipFollowUp(task?.status, { credits: Boolean(failureView?.credits) });
    if (followUp === 'regenerate') {
      await regenerate();
      return;
    }
    if (!task) setLoading(true);
    setRawError(null);
    setStatusErrorKey(null);
    setPollEpoch((epoch) => epoch + 1);
  }, [task, failureView?.credits, regenerate]);

  const handleOpenFolder = useCallback(async () => {
    if (!task?.result_media_id) return;
    setIsOpeningFolder(true);
    try {
      const localPath = await getMediaPath(task.result_media_id);
      await ipcBridge.shell.showItemInFolder.invoke(localPath);
    } catch (err) {
      message.error(
        t('videoGeneration.clip.openFolderFailed', {
          error: err instanceof globalThis.Error ? err.message : String(err),
        })
      );
    } finally {
      setIsOpeningFolder(false);
    }
  }, [task, t, message]);

  const revealControls = useCallback(() => {
    setShowControls(true);
    if (controlsTimerRef.current) clearTimeout(controlsTimerRef.current);
    controlsTimerRef.current = setTimeout(() => {
      const video = videoRef.current;
      if (video && !video.paused) setShowControls(false);
    }, 2800);
  }, []);

  const seekFromPointer = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
    const video = videoRef.current;
    const length = video?.duration;
    if (!video || !length || !Number.isFinite(length)) return;
    const rect = event.currentTarget.getBoundingClientRect();
    const ratio = rect.width > 0 ? (event.clientX - rect.left) / rect.width : 0;
    video.currentTime = Math.min(length, Math.max(0, ratio * length));
  }, []);

  useEffect(() => {
    return subscribeFullscreenChange(() => {
      const native = isElementFullscreen(playerRef.current);
      if (native) {
        cssFullscreenRef.current = false;
        setUseCssFullscreen(false);
        setIsFullscreen(true);
        return;
      }
      if (!cssFullscreenRef.current) setIsFullscreen(false);
    });
  }, []);

  useEffect(() => {
    if (!isFullscreen) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      void exitElementFullscreen();
      cssFullscreenRef.current = false;
      setUseCssFullscreen(false);
      setIsFullscreen(false);
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [isFullscreen]);

  const isRunning = task?.status === 'queued' || task?.status === 'running';
  const isSucceeded = task?.status === 'succeeded';

  useEffect(() => {
    if (!isRunning) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [isRunning]);

  const promptText = (task?.prompt || prompt || '').trim();
  const displayTitle =
    title?.trim() || task?.prompt?.trim().slice(0, 48) || t('videoGeneration.clip.defaultTitle');
  const aspect = clipFrameAspect(videoAspect, task?.aspect_ratio);
  const percent = clipProgressPercent(task?.progress);
  const createdMs = task ? clipTimestampMs(task.created_at) : null;
  const elapsedSecs = createdMs ? Math.max(0, Math.floor((now - createdMs) / 1000)) : 0;
  const followUp = clipFollowUp(task?.status, { credits: Boolean(failureView?.credits) });
  const retryLabel =
    followUp === 'regenerate'
      ? t('videoGeneration.clip.regenerate')
      : t('videoGeneration.clip.retry');
  const stills = task ? clipStillIds(task) : [];
  const modelLabel = shortClipModel(task?.model);
  const controlsOn = !isPlaying || showControls;
  const missingMedia = Boolean(isSucceeded && !videoUrl && !failureView);
  const shownFailure: ClipFailureView | null =
    failureView ??
    (missingMedia
      ? {
          credits: false,
          title: t('videoGeneration.clip.errorTitle'),
          message: t('videoGeneration.clip.missingMedia'),
          recovery: null,
        }
      : null);

  const progressFraction = duration > 0 ? currentTime / duration : 0;
  const bufferedFraction = duration > 0 ? buffered / duration : 0;

  const createdLabel =
    createdMs == null
      ? ''
      : new Intl.DateTimeFormat(i18n.language, {
          month: 'short',
          day: 'numeric',
          hour: '2-digit',
          minute: '2-digit',
        }).format(createdMs);

  const statusLabel = (() => {
    switch (task?.status) {
      case 'queued':
        return t('videoGeneration.clip.status.queued');
      case 'running':
        return t('videoGeneration.clip.status.running');
      case 'succeeded':
        return t('videoGeneration.clip.status.succeeded');
      case 'failed':
        return t('videoGeneration.clip.status.failed');
      case 'canceled':
        return t('videoGeneration.clip.status.canceled');
      default:
        return '';
    }
  })();

  const stepLabel = (() => {
    switch (clipStepKey(task?.status, percent)) {
      case 'initializing':
        return t('videoGeneration.clip.step.initializing');
      case 'rendering':
        return t('videoGeneration.clip.step.rendering');
      case 'finalizing':
        return t('videoGeneration.clip.step.finalizing');
      default:
        return t('videoGeneration.clip.step.queued');
    }
  })();

  const runningHint =
    task?.status === 'queued'
      ? t('videoGeneration.clip.status.queuedDesc')
      : percent != null
        ? t('videoGeneration.clip.progress', { percent })
        : t('videoGeneration.clip.generatingHint');

  return (
    <div className={[styles.page, 'page flex-1 min-h-0 size-full box-border overflow-y-auto'].join(' ')}>
      {messageHolder}

      <header className={styles.stickyHeader}>
        <div className={styles.headerInner}>
          <Button
            shape='round'
            type='outline'
            size='small'
            className='flowy-icon-text-btn'
            icon={<ArrowLeft theme='outline' size={14} />}
            onClick={handleBack}
          >
            {t('videoGeneration.clip.back')}
          </Button>
          <div className={styles.headerCopy}>
            <h1 className={styles.title} title={promptText || displayTitle}>
              {displayTitle}
            </h1>
            {task ? (
              <span className={styles.status} data-tone={statusTone(task.status)}>
                <span className={styles.statusDot} />
                {statusLabel}
              </span>
            ) : null}
          </div>
        </div>
      </header>

      <main className={styles.content}>
        <div className={styles.stage}>
          {loading && !task && !shownFailure ? (
            <div className={styles.frame} style={frameStyle(16 / 9)}>
              <LoadingOne theme='outline' size={28} className={styles.spinning} />
              <p className={styles.loadingTitle}>{t('videoGeneration.clip.loading')}</p>
              <p className={styles.loadingHint}>{t('videoGeneration.clip.loadingHint')}</p>
            </div>
          ) : null}

          {shownFailure && !task ? (
            <ClipFailurePanel
              view={shownFailure}
              layout='page'
              onRetry={() => void handleRetry()}
              onBilling={handleOpenBilling}
              retryLabel={retryLabel}
              billingLabel={t('billing.openBilling', { defaultValue: '购买积分' })}
              busy={isRegenerating}
            />
          ) : null}

          {task ? (
            <>
              {isSucceeded && videoUrl ? (
                <div className={styles.playerColumn} style={frameStyle(aspect)}>
                <div
                  ref={playerRef}
                  className={[
                    styles.videoContainer,
                    isFullscreen ? styles.videoFullscreen : '',
                    useCssFullscreen ? styles.videoCssFullscreen : '',
                  ]
                    .filter(Boolean)
                    .join(' ')}
                  style={isFullscreen ? undefined : frameStyle(aspect)}
                  onMouseMove={revealControls}
                  onMouseLeave={() => {
                    const player = playerRef.current;
                    const active = document.activeElement;
                    if (player && active instanceof Node && player.contains(active)) return;
                    if (videoRef.current && !videoRef.current.paused) setShowControls(false);
                  }}
                >
                  <video
                    ref={videoRef}
                    className={styles.videoPlayer}
                    src={videoUrl}
                    controls={false}
                    playsInline
                    preload='metadata'
                    muted={muted}
                    onPlay={() => setIsPlaying(true)}
                    onPause={() => {
                      setIsPlaying(false);
                      setShowControls(true);
                    }}
                    onEnded={() => {
                      setIsPlaying(false);
                      setShowControls(true);
                    }}
                    onLoadedMetadata={(event) => {
                      const el = event.currentTarget;
                      if (el.videoWidth > 0 && el.videoHeight > 0) {
                        setVideoAspect(el.videoWidth / el.videoHeight);
                      }
                      setDuration(el.duration);
                    }}
                    onTimeUpdate={() => {
                      if (videoRef.current) setCurrentTime(videoRef.current.currentTime);
                    }}
                    onDurationChange={() => {
                      if (videoRef.current) setDuration(videoRef.current.duration);
                    }}
                    onProgress={() => {
                      const video = videoRef.current;
                      if (video && video.buffered.length > 0) {
                        setBuffered(video.buffered.end(video.buffered.length - 1));
                      }
                    }}
                    onClick={handlePlayPause}
                  />

                  <div
                    className={[styles.videoControls, controlsOn ? styles.videoControlsVisible : '']
                      .filter(Boolean)
                      .join(' ')}
                  >
                    <div
                      className={styles.scrubber}
                      role='slider'
                      tabIndex={0}
                      aria-label={t('videoGeneration.clip.seek')}
                      aria-valuemin={0}
                      aria-valuemax={Math.round(duration) || 0}
                      aria-valuenow={Math.round(currentTime) || 0}
                      onPointerDown={(event) => {
                        event.currentTarget.setPointerCapture(event.pointerId);
                        seekFromPointer(event);
                      }}
                      onPointerMove={(event) => {
                        if (!event.currentTarget.hasPointerCapture(event.pointerId)) return;
                        seekFromPointer(event);
                      }}
                      onKeyDown={(event) => {
                        const video = videoRef.current;
                        if (!video) return;
                        if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
                          event.preventDefault();
                          event.stopPropagation();
                          const delta = event.key === 'ArrowLeft' ? -5 : 5;
                          video.currentTime = Math.min(
                            video.duration || 0,
                            Math.max(0, video.currentTime + delta)
                          );
                        }
                      }}
                    >
                      <div className={styles.scrubberTrack}>
                        <div
                          className={styles.scrubberBuffer}
                          style={{ width: `${bufferedFraction * 100}%` }}
                        />
                        <div
                          className={styles.scrubberProgress}
                          style={{ width: `${progressFraction * 100}%` }}
                        />
                        <div
                          className={styles.scrubberThumb}
                          style={{ left: `${progressFraction * 100}%` }}
                        />
                      </div>
                    </div>

                    <div className={styles.controlsBar}>
                      <div className={styles.controlsLeft}>
                        <button
                          type='button'
                          className={styles.controlBtn}
                          onClick={handlePlayPause}
                          aria-label={
                            isPlaying
                              ? t('videoGeneration.clip.pause')
                              : t('videoGeneration.clip.play')
                          }
                        >
                          {isPlaying ? (
                            <PauseOne theme='outline' size={20} />
                          ) : (
                            <PlayOne theme='outline' size={20} />
                          )}
                        </button>
                        <div className={styles.volumeControl}>
                          <button
                            type='button'
                            className={styles.controlBtn}
                            onClick={() => setMuted((value) => !value)}
                            aria-label={
                              muted ? t('videoGeneration.clip.unmute') : t('videoGeneration.clip.mute')
                            }
                          >
                            {muted || volume === 0 ? (
                              <VolumeMute theme='outline' size={18} />
                            ) : (
                              <VolumeNotice theme='outline' size={18} />
                            )}
                          </button>
                          <input
                            type='range'
                            min={0}
                            max={1}
                            step={0.02}
                            value={muted ? 0 : volume}
                            onChange={(event) => {
                              const next = Number.parseFloat(event.target.value);
                              setVolume(next);
                              setMuted(next === 0);
                            }}
                            className={styles.volumeSlider}
                            aria-label={t('videoGeneration.clip.volume')}
                          />
                        </div>
                        <span className={styles.timeDisplay}>
                          <RollingDuration
                            seconds={currentTime}
                            hours='never'
                            padMinutes={false}
                            locale={i18n.language}
                          />
                          <span className={styles.timeDivider}>/</span>
                          <RollingDuration
                            seconds={duration}
                            hours='never'
                            padMinutes={false}
                            locale={i18n.language}
                          />
                        </span>
                      </div>
                      <div className={styles.controlsRight}>
                        <button
                          type='button'
                          className={styles.controlBtn}
                          data-testid='clip-player-fullscreen'
                          onClick={(event) => {
                            event.stopPropagation();
                            handleFullscreen();
                          }}
                          aria-label={
                            isFullscreen
                              ? t('videoGeneration.clip.exitFullscreen')
                              : t('videoGeneration.clip.fullscreen')
                          }
                          title={
                            isFullscreen
                              ? t('videoGeneration.clip.exitFullscreen')
                              : t('videoGeneration.clip.fullscreen')
                          }
                        >
                          {isFullscreen ? (
                            <OffScreen theme='outline' size={18} />
                          ) : (
                            <FullScreen theme='outline' size={18} />
                          )}
                        </button>
                      </div>
                    </div>
                  </div>

                  {!isPlaying ? (
                    <button
                      type='button'
                      className={styles.bigPlayBtn}
                      onClick={handlePlayPause}
                      aria-label={t('videoGeneration.clip.play')}
                    >
                      <PlayOne theme='filled' size={28} />
                    </button>
                  ) : null}
                </div>
                <div className={styles.toolbar}>
                  <Button
                    shape='round'
                    type='primary'
                    className='flowy-icon-text-btn'
                    icon={<FolderOpen theme='outline' size={14} />}
                    loading={isOpeningFolder}
                    disabled={isOpeningFolder}
                    onClick={() => void handleOpenFolder()}
                  >
                    {t('videoGeneration.clip.openFolder')}
                  </Button>
                  <Button
                    shape='round'
                    type='outline'
                    className='flowy-icon-text-btn'
                    icon={<Refresh theme='outline' size={14} />}
                    loading={isRegenerating}
                    disabled={isRegenerating}
                    onClick={() => void handleRetry()}
                  >
                    {t('videoGeneration.clip.regenerate')}
                  </Button>
                </div>
                </div>
              ) : isRunning && !shownFailure ? (
                <div className={styles.frame} style={frameStyle(aspect)} role='status'>
                  {task.status === 'running' ? (
                    <p className={styles.generatingKicker}>{stepLabel}</p>
                  ) : null}
                  <p className={styles.generatingTitle}>
                    {task.status === 'queued'
                      ? t('videoGeneration.clip.status.queued')
                      : t('videoGeneration.clip.generating')}
                  </p>
                  <p className={styles.generatingHint}>{runningHint}</p>
                  <p className={styles.elapsed}>
                    <TimedPhrase
                      text={t('videoGeneration.clip.elapsed', { time: timerSlot.time })}
                      slots={{
                        [timerSlot.time]: (
                          <RollingDuration
                            seconds={elapsedSecs}
                            hours='auto'
                            locale={i18n.language}
                          />
                        ),
                      }}
                    />
                  </p>
                  <div
                    className={styles.progressTrack}
                    role='progressbar'
                    aria-valuemin={0}
                    aria-valuemax={100}
                    aria-valuenow={percent ?? undefined}
                    aria-label={t('videoGeneration.clip.generating')}
                  >
                    <div
                      className={percent == null ? styles.progressIndeterminate : styles.progressBar}
                      style={percent == null ? undefined : { width: `${percent}%` }}
                    />
                  </div>
                </div>
              ) : shownFailure ? (
                <ClipFailurePanel
                  view={shownFailure}
                  layout='panel'
                  onRetry={() => void handleRetry()}
                  onBilling={handleOpenBilling}
                  retryLabel={retryLabel}
                  billingLabel={t('billing.openBilling', { defaultValue: '购买积分' })}
                  busy={isRegenerating}
                />
              ) : null}

              {promptText ? (
                <section className={styles.promptBlock}>
                  <div className={styles.promptHead}>
                    <h2 className={styles.promptLabel}>{t('videoGeneration.clip.prompt')}</h2>
                    <button
                      type='button'
                      className={styles.textAction}
                      onClick={() => void copyText(promptText, 'prompt')}
                    >
                      {copied === 'prompt' ? (
                        <Check theme='outline' size={14} />
                      ) : (
                        <Copy theme='outline' size={14} />
                      )}
                      <span>
                        {copied === 'prompt'
                          ? t('videoGeneration.clip.copied')
                          : t('videoGeneration.clip.copyPrompt')}
                      </span>
                    </button>
                  </div>
                  <p className={styles.promptBody}>{promptText}</p>
                </section>
              ) : null}

              {stills.length > 0 ? (
                <ReferenceStills ids={stills} label={t('videoGeneration.clip.references')} />
              ) : null}

              <ul className={styles.specs}>
                {modelLabel ? (
                  <li className={styles.spec} title={task?.model || modelLabel}>
                    <span className={styles.specLabel}>{t('videoGeneration.clip.model')}</span>
                    <span>{modelLabel}</span>
                  </li>
                ) : null}
                {task.aspect_ratio ? (
                  <li className={styles.spec}>
                    <span className={styles.specLabel}>{t('videoGeneration.clip.aspect')}</span>
                    <span>{task.aspect_ratio}</span>
                  </li>
                ) : null}
                {task.resolution ? (
                  <li className={styles.spec}>
                    <span className={styles.specLabel}>{t('videoGeneration.clip.resolution')}</span>
                    <span>{task.resolution}</span>
                  </li>
                ) : null}
                {task.duration_secs ? (
                  <li className={styles.spec}>
                    <span className={styles.specLabel}>{t('videoGeneration.clip.duration')}</span>
                    <span>
                      {t('videoGeneration.clip.durationValue', { count: task.duration_secs })}
                    </span>
                  </li>
                ) : null}
                {createdLabel ? (
                  <li className={styles.spec}>
                    <span className={styles.specLabel}>{t('videoGeneration.clip.createdAt')}</span>
                    <time dateTime={createdMs ? new Date(createdMs).toISOString() : undefined}>
                      {createdLabel}
                    </time>
                  </li>
                ) : null}
                {taskId ? (
                  <li>
                    <button
                      type='button'
                      className={styles.specButton}
                      title={taskId}
                      aria-label={t('videoGeneration.clip.copyId')}
                      onClick={() => void copyText(taskId, 'id')}
                    >
                      <span className={styles.specLabel}>ID</span>
                      <span className={styles.specMono}>{taskId.slice(0, 8)}</span>
                      {copied === 'id' ? (
                        <Check theme='outline' size={12} />
                      ) : (
                        <Copy theme='outline' size={12} />
                      )}
                    </button>
                  </li>
                ) : null}
              </ul>
            </>
          ) : null}

          {!task && promptText && !loading ? (
            <section className={styles.promptBlock}>
              <h2 className={styles.promptLabel}>{t('videoGeneration.clip.prompt')}</h2>
              <p className={styles.promptBody}>{promptText}</p>
            </section>
          ) : null}
        </div>
      </main>
    </div>
  );
};

export default ClipResultPage;
