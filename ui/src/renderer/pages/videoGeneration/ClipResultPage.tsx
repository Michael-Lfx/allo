/**
 * Video clip generation result page (`/video-generation/clip/:taskId`).
 *
 * This is the dedicated result page for the "视频生成" (clip generate) mode.
 * It polls the generation task status and displays the final video when ready.
 */
import React, {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import { useLocation, useNavigate, useParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Button } from '@arco-design/web-react';
import {
  ArrowLeft,
  Refresh,
  Error,
  LoadingOne,
  FullScreen,
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
import styles from './ClipResultPage.module.css';

type ClipStatusErrorKey =
  | 'taskNotFound'
  | 'generationCanceled'
  | 'generationTimeout';

const POLL_INTERVAL_MS = 2000;
const MAX_POLL_ATTEMPTS = 300; // 10 minutes max

type LocationState = {
  title?: string;
  prompt?: string;
  taskId?: string;
};

function formatDuration(secs: number): string {
  const m = Math.floor(secs / 60);
  const s = Math.floor(secs % 60);
  return `${m}:${s.toString().padStart(2, '0')}`;
}

function ClipFailurePanel({
  view,
  layout,
  onBack,
  onRetry,
  onBilling,
  backLabel,
  retryLabel,
  billingLabel,
}: {
  view: ClipFailureView;
  layout: 'page' | 'panel';
  onBack?: () => void;
  onRetry: () => void;
  onBilling?: () => void;
  backLabel: string;
  retryLabel: string;
  billingLabel: string;
}) {
  const billingButton = view.recovery ? (
    <Button
      shape='round'
      type='primary'
      data-testid='video-failure-open-billing'
      onClick={onBilling}
    >
      {billingLabel}
    </Button>
  ) : null;

  if (layout === 'panel') {
    return (
      <div className={styles.errorPanel} role='alert'>
        <div className={styles.errorPanelHeader}>
          <Error theme='outline' size={18} className={styles.errorBannerIcon} />
          <h2 className={styles.errorPanelTitle}>{view.title}</h2>
        </div>
        <p className={styles.errorPanelMessage}>{view.message}</p>
        {billingButton ? <div className={styles.errorPanelActions}>{billingButton}</div> : null}
      </div>
    );
  }

  return (
    <div className={styles.errorState} role='alert'>
      <div className={styles.errorIcon}>
        <Error theme='outline' size={40} />
      </div>
      <h2 className={styles.errorTitle}>{view.title}</h2>
      <p className={styles.errorMessage}>{view.message}</p>
      <div className={styles.errorActions}>
        {onBack ? (
          <Button shape='round' type='outline' onClick={onBack}>
            <ArrowLeft theme='outline' size={14} />
            {backLabel}
          </Button>
        ) : null}
        {billingButton}
        <Button shape='round' type={view.recovery ? 'outline' : 'primary'} onClick={onRetry}>
          <Refresh theme='outline' size={14} />
          {retryLabel}
        </Button>
      </div>
    </div>
  );
}

const ClipResultPage: React.FC = () => {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const location = useLocation();
  const { taskId: urlTaskId } = useParams<{ taskId: string }>();
  const { logout } = useCloudAuth();
  const { balance } = useCredits();
  const [message, messageHolder] = useArcoMessage();

  const taskId = urlTaskId || (location.state as LocationState)?.taskId;
  const title = (location.state as LocationState)?.title;
  const prompt = (location.state as LocationState)?.prompt;

  const [task, setTask] = useState<GenerationTaskView | null>(null);
  const [loading, setLoading] = useState(true);
  const [rawError, setRawError] = useState<string | null>(null);
  const [statusErrorKey, setStatusErrorKey] = useState<ClipStatusErrorKey | null>(
    null
  );
  const [videoUrl, setVideoUrl] = useState<string | null>(null);
  const [isPlaying, setIsPlaying] = useState(false);
  const [isFullscreen, setIsFullscreen] = useState(false);
  const [muted, setMuted] = useState(false);
  const [volume, setVolume] = useState(1);
  const [currentTime, setCurrentTime] = useState(0);
  const [duration, setDuration] = useState(0);
  const [buffered, setBuffered] = useState(0);
  const [showControls, setShowControls] = useState(true);
  const [isOpeningFolder, setIsOpeningFolder] = useState(false);
  const [isRegenerating, setIsRegenerating] = useState(false);
  const [copied, setCopied] = useState(false);
  // Detected aspect ratio of the loaded video (width / height). Drives the
  // preview frame so the container matches the video's intrinsic ratio and
  // there are no letterbox bars around the actual footage.
  const [videoAspect, setVideoAspect] = useState<number | null>(null);

  const pollAttemptsRef = useRef(0);
  const videoRef = useRef<HTMLVideoElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const controlsTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const fetchTask = useCallback(async () => {
    if (!taskId) {
      setStatusErrorKey('taskNotFound');
      setRawError(null);
      return true;
    }
    try {
      const result = await getGenerationTask(taskId);
      setTask(result);
      setStatusErrorKey(null);

      // Track this clip task in the MRU strip
      const resolvedTitle =
        title?.trim() || result.prompt?.trim().slice(0, 48);
      rememberVideoGenerationTask(taskId, resolvedTitle);

      if (result.status === 'succeeded') {
        setRawError(null);
        if (result.result_media_id) {
          setVideoUrl(canvasMediaUrl(result.result_media_id));
        }
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
      const errorMessage = (e as globalThis.Error).message ?? String(e);
      console.error('[ClipResultPage] failed to fetch task:', errorMessage);
      setStatusErrorKey(null);
      setRawError(errorMessage);
      return true;
    }
  }, [taskId, logout, navigate, title]);

  // Start timer when task starts running
  useEffect(() => {
    let pollInterval: ReturnType<typeof setInterval> | null = null;

    const startPolling = async () => {
      pollAttemptsRef.current = 0;
      setVideoUrl(null);
      setVideoAspect(null);
      setIsPlaying(false);
      setCurrentTime(0);
      setDuration(0);
      setLoading(true);
      const done = await fetchTask();
      setLoading(false);

      if (!done) {
        pollInterval = setInterval(async () => {
          pollAttemptsRef.current += 1;
          if (pollAttemptsRef.current >= MAX_POLL_ATTEMPTS) {
            if (pollInterval) clearInterval(pollInterval);
            setStatusErrorKey('generationTimeout');
            setRawError(null);
            return;
          }
          const finished = await fetchTask();
          if (finished && pollInterval) {
            clearInterval(pollInterval);
          }
        }, POLL_INTERVAL_MS);
      }
    };

    void startPolling();

    return () => {
      if (pollInterval) clearInterval(pollInterval);
    };
  }, [fetchTask]);

  // Keyboard shortcuts
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      const video = videoRef.current;
      if (!video) return;

      // Only when not typing in an input
      if (
        e.target instanceof HTMLInputElement ||
        e.target instanceof HTMLTextAreaElement
      ) {
        return;
      }

      switch (e.key) {
        case ' ':
        case 'k':
          e.preventDefault();
          handlePlayPause();
          break;
        case 'ArrowLeft':
          e.preventDefault();
          video.currentTime = Math.max(0, video.currentTime - 5);
          break;
        case 'ArrowRight':
          e.preventDefault();
          video.currentTime = Math.min(
            video.duration,
            video.currentTime + 5
          );
          break;
        case 'ArrowUp':
          e.preventDefault();
          setVolume((v) => Math.min(1, v + 0.1));
          break;
        case 'ArrowDown':
          e.preventDefault();
          setVolume((v) => Math.max(0, v - 0.1));
          break;
        case 'm':
          setMuted((m) => !m);
          break;
        case 'f':
          handleFullscreen();
          break;
      }
    };

    document.addEventListener('keydown', handleKeyDown);
    return () => document.removeEventListener('keydown', handleKeyDown);
  }, []);

  // Sync volume to video
  useEffect(() => {
    const video = videoRef.current;
    if (video) {
      video.volume = volume;
      video.muted = muted;
    }
  }, [volume, muted]);

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

  const handleRetry = useCallback(async () => {
    if (task?.status === 'succeeded') {
      const promptText = (task.prompt || prompt || '').trim();
      if (!promptText) {
        message.error(
          t('videoGeneration.clip.regenerateNoPrompt', {
            defaultValue: '没有可用的提示词，无法重新生成',
          })
        );
        return;
      }
      setIsRegenerating(true);
      try {
        const next = await createGenerationTask({
          mode: task.mode || 'video',
          prompt: promptText,
          model: task.model || undefined,
          aspect_ratio: task.aspect_ratio || undefined,
          resolution: task.resolution || undefined,
          duration_secs: task.duration_secs || undefined,
          reference_media_ids: task.reference_media_ids,
          first_frame_media_id: task.first_frame_media_id || undefined,
          last_frame_media_id: task.last_frame_media_id || undefined,
        });
        navigate(`/video-generation/clip/${encodeURIComponent(next.task_id)}`, {
          replace: true,
          state: {
            title: title || promptText.slice(0, 48),
            prompt: promptText,
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
      return;
    }

    setRawError(null);
    setStatusErrorKey(null);
    setTask(null);
    setVideoUrl(null);
    setVideoAspect(null);
    pollAttemptsRef.current = 0;
    setIsPlaying(false);
    setCurrentTime(0);
    setDuration(0);
    void fetchTask();
  }, [task, prompt, title, navigate, fetchTask, message, t, balance]);

  const handleOpenFolder = useCallback(async () => {
    if (!task?.result_media_id) return;

    setIsOpeningFolder(true);
    try {
      const localPath = await getMediaPath(task.result_media_id);
      await ipcBridge.shell.showItemInFolder.invoke(localPath);
    } catch (err) {
      message.error(
        t('videoGeneration.clip.openFolderFailed', {
          defaultValue: `无法打开所在位置：${err instanceof globalThis.Error ? err.message : String(err)}`,
        })
      );
    } finally {
      setIsOpeningFolder(false);
    }
  }, [task, t, message]);

  const handleCopyId = useCallback(() => {
    if (!taskId) return;
    void navigator.clipboard.writeText(taskId);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  }, [taskId]);

  const handlePlayPause = useCallback(() => {
    const video = videoRef.current;
    if (!video) return;
    if (isPlaying) {
      video.pause();
    } else {
      void video.play();
    }
  }, [isPlaying]);

  const handleFullscreen = useCallback(() => {
    if (!containerRef.current) return;
    if (isFullscreen) {
      document.exitFullscreen?.();
    } else {
      void containerRef.current.requestFullscreen?.();
    }
  }, [isFullscreen]);

  const handleSeek = useCallback((e: React.MouseEvent<HTMLDivElement>) => {
    const video = videoRef.current;
    if (!video || !duration) return;
    const rect = e.currentTarget.getBoundingClientRect();
    const ratio = (e.clientX - rect.left) / rect.width;
    video.currentTime = ratio * duration;
  }, [duration]);

  const handleVolumeChange = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
    const val = parseFloat(e.target.value);
    setVolume(val);
    if (val === 0) setMuted(true);
    else if (muted) setMuted(false);
  }, [muted]);

  const handleMouseMove = useCallback(() => {
    setShowControls(true);
    if (controlsTimerRef.current) {
      clearTimeout(controlsTimerRef.current);
    }
    controlsTimerRef.current = setTimeout(() => {
      if (isPlaying) setShowControls(false);
    }, 3000);
  }, [isPlaying]);

  useEffect(() => {
    const handleFullscreenChange = () => {
      setIsFullscreen(!!document.fullscreenElement);
    };
    document.addEventListener('fullscreenchange', handleFullscreenChange);
    return () =>
      document.removeEventListener('fullscreenchange', handleFullscreenChange);
  }, []);

  const isRunning = task?.status === 'queued' || task?.status === 'running';
  const isSucceeded = task?.status === 'succeeded';

  const progressFraction = duration > 0 ? currentTime / duration : 0;
  const bufferedFraction = duration > 0 ? buffered / duration : 0;

  const displayTitle =
    title?.trim() || task?.prompt?.slice(0, 48) || t('videoGeneration.clip.defaultTitle');

  return (
    <div
      ref={containerRef}
      className={`${styles.page} page`}
      onMouseMove={isSucceeded ? handleMouseMove : undefined}
      onMouseLeave={isSucceeded ? () => setShowControls(false) : undefined}
    >
      {messageHolder}

      {/* Sticky Header */}
      <div className={styles.stickyHeader}>
        <div className={styles.headerInner}>
          <Button
            shape='round'
            type='outline'
            size='small'
            onClick={handleBack}
            className={styles.backBtn}
          >
            <ArrowLeft theme='outline' size={14} />
            {t('videoGeneration.clip.back')}
          </Button>

          <div className={styles.headerTitle}>
            <h1 className={styles.title}>{displayTitle}</h1>
            {taskId && (
              <button
                type='button'
                className={styles.taskIdBadge}
                onClick={handleCopyId}
                title={t('videoGeneration.clip.copyId', { defaultValue: '复制任务 ID' })}
              >
                <span className={styles.taskIdText}>{taskId.slice(0, 8)}...</span>
                {copied ? (
                  <Check theme='outline' size={11} className={styles.copyIcon} />
                ) : (
                  <Copy theme='outline' size={11} className={styles.copyIcon} />
                )}
              </button>
            )}
          </div>
        </div>
      </div>

      {/* Main Content */}
      <div className={styles.content}>
        {/* Initial loading: connecting to server */}
        {loading && !task && (
          <div className={styles.loadingState}>
            <div className={styles.loadingIcon}>
              <LoadingOne theme='outline' size={48} className={styles.spinning} />
            </div>
            <p className={styles.loadingText}>
              {t('videoGeneration.clip.loading')}
            </p>
            <p className={styles.loadingHint}>
              {t('videoGeneration.clip.loadingHint', {
                defaultValue: '正在连接到服务器...',
              })}
            </p>
          </div>
        )}

        {failureView && !task && (
          <ClipFailurePanel
            view={failureView}
            layout='page'
            onBack={handleBack}
            onRetry={() => void handleRetry()}
            onBilling={handleOpenBilling}
            backLabel={t('videoGeneration.clip.back')}
            retryLabel={t('videoGeneration.clip.retry')}
            billingLabel={t('billing.openBilling', { defaultValue: '购买积分' })}
          />
        )}

        {task && (
          <div className={styles.layout}>
            {/* Left: Video / Progress */}
            <div className={styles.mainColumn}>
              {/* Video Player or Generating Placeholder */}
              {isSucceeded && videoUrl ? (
                <div
                  className={`${styles.videoContainer} ${
                    isFullscreen ? styles.videoFullscreen : ''
                  }`}
                  style={
                    videoAspect
                      ? { aspectRatio: `${videoAspect}` }
                      : undefined
                  }
                >
                  <video
                    ref={videoRef}
                    className={styles.videoPlayer}
                    src={videoUrl}
                    controls={false}
                    playsInline
                    muted={muted}
                    onPlay={() => setIsPlaying(true)}
                    onPause={() => setIsPlaying(false)}
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
                      if (videoRef.current) {
                        setCurrentTime(videoRef.current.currentTime);
                      }
                    }}
                    onDurationChange={() => {
                      if (videoRef.current) {
                        setDuration(videoRef.current.duration);
                      }
                    }}
                    onProgress={() => {
                      const video = videoRef.current;
                      if (video && video.buffered.length > 0) {
                        setBuffered(video.buffered.end(video.buffered.length - 1));
                      }
                    }}
                    onClick={handlePlayPause}
                  />

                  {/* Video Controls Overlay */}
                  <div
                    className={`${styles.videoControls} ${
                      showControls ? styles.videoControlsVisible : ''
                    }`}
                  >
                    {/* Scrubber */}
                    <div className={styles.scrubber} onClick={handleSeek}>
                      <div className={styles.scrubberBuffer} style={{ width: `${bufferedFraction * 100}%` }} />
                      <div
                        className={styles.scrubberProgress}
                        style={{ width: `${progressFraction * 100}%` }}
                      />
                      <div
                        className={styles.scrubberThumb}
                        style={{ left: `${progressFraction * 100}%` }}
                      />
                    </div>

                    <div className={styles.controlsBar}>
                      {/* Left controls */}
                      <div className={styles.controlsLeft}>
                        <button
                          className={styles.controlBtn}
                          onClick={handlePlayPause}
                          aria-label={isPlaying ? '暂停' : '播放'}
                        >
                          {isPlaying ? (
                            <PauseOne theme='outline' size={20} />
                          ) : (
                            <PlayOne theme='outline' size={20} />
                          )}
                        </button>

                        {/* Volume */}
                        <div className={styles.volumeControl}>
                          <button
                            className={styles.controlBtn}
                            onClick={() => setMuted((m) => !m)}
                            aria-label={muted ? '取消静音' : '静音'}
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
                            onChange={handleVolumeChange}
                            className={styles.volumeSlider}
                            aria-label='音量'
                          />
                        </div>

                        <span className={styles.timeDisplay}>
                          {formatDuration(currentTime)} / {formatDuration(duration)}
                        </span>
                      </div>

                      {/* Right controls */}
                      <div className={styles.controlsRight}>
                        <button
                          className={styles.controlBtn}
                          onClick={handleFullscreen}
                          aria-label='全屏'
                        >
                          <FullScreen theme='outline' size={18} />
                        </button>
                      </div>
                    </div>
                  </div>

                  {/* Big play button overlay when paused */}
                  {!isPlaying && (
                    <button
                      className={styles.bigPlayBtn}
                      onClick={handlePlayPause}
                      aria-label='播放'
                    >
                      <PlayOne theme='outline' size={40} fill='currentColor' />
                    </button>
                  )}
                </div>
              ) : isRunning && !failureView ? (
                <div className={styles.videoPlaceholder}>
                  <div className={styles.placeholderSpinner}>
                    <LoadingOne theme='outline' size={32} className={styles.spinning} />
                  </div>
                  <p className={styles.placeholderTitle}>
                    {t('videoGeneration.clip.generating', {
                      defaultValue: '视频正在生成',
                    })}
                  </p>
                </div>
              ) : failureView ? (
                <ClipFailurePanel
                  view={failureView}
                  layout='panel'
                  onRetry={() => void handleRetry()}
                  onBilling={handleOpenBilling}
                  backLabel={t('videoGeneration.clip.back')}
                  retryLabel={t('videoGeneration.clip.retry')}
                  billingLabel={t('billing.openBilling', { defaultValue: '购买积分' })}
                />
              ) : null}
            </div>

            {/* Right: Info Sidebar */}
            <div className={styles.sidebar}>
              <div className={styles.actionList}>
                {isSucceeded && (
                  <button
                    type='button'
                    className={styles.actionItem}
                    disabled={isOpeningFolder}
                    onClick={() => void handleOpenFolder()}
                  >
                    {isOpeningFolder ? (
                      <LoadingOne theme='outline' size={16} className={styles.spinning} />
                    ) : (
                      <FolderOpen theme='outline' size={16} />
                    )}
                    <span>
                      {t('videoGeneration.clip.openFolder', {
                        defaultValue: '打开视频所在位置',
                      })}
                    </span>
                  </button>
                )}
                <button
                  type='button'
                  className={styles.actionItem}
                  disabled={isRegenerating}
                  onClick={() => void handleRetry()}
                >
                  {isRegenerating ? (
                    <LoadingOne theme='outline' size={16} className={styles.spinning} />
                  ) : (
                    <Refresh theme='outline' size={16} />
                  )}
                  <span>
                    {isSucceeded
                      ? t('videoGeneration.clip.regenerate')
                      : t('videoGeneration.clip.refresh')}
                  </span>
                </button>
              </div>

              <dl className={styles.metaList}>
                {(task?.prompt || prompt) && (
                  <div className={styles.metaRow}>
                    <dt>
                      {t('videoGeneration.clip.prompt', { defaultValue: '提示词' })}
                    </dt>
                    <dd>{task?.prompt || prompt}</dd>
                  </div>
                )}
                {task?.model && (
                  <div className={styles.metaRow}>
                    <dt>
                      {t('videoGeneration.clip.model', { defaultValue: '模型' })}
                    </dt>
                    <dd>{task.model}</dd>
                  </div>
                )}
                {task?.mode && (
                  <div className={styles.metaRow}>
                    <dt>
                      {t('videoGeneration.clip.mode', { defaultValue: '模式' })}
                    </dt>
                    <dd>{task.mode}</dd>
                  </div>
                )}
                {task?.created_at && (
                  <div className={styles.metaRow}>
                    <dt>
                      {t('videoGeneration.clip.createdAt', { defaultValue: '创建时间' })}
                    </dt>
                    <dd>{new Date(task.created_at * 1000).toLocaleString()}</dd>
                  </div>
                )}
                <div className={styles.metaRow}>
                  <dt>ID</dt>
                  <dd className={styles.metaMono}>{taskId}</dd>
                </div>
              </dl>
            </div>
          </div>
        )}
      </div>
    </div>
  );
};

export default ClipResultPage;
