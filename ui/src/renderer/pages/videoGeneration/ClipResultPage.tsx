/**
 * Video clip generation result page (`/video-generation/clip/:taskId`).
 *
 * This is the dedicated result page for the "视频生成" (clip generate) mode.
 * It polls the generation task status and displays the final video when ready.
 */
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useLocation, useNavigate, useParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';
import { Button, Progress, Result, Spin } from '@arco-design/web-react';
import {
  ArrowLeft,
  Download,
  Refresh,
  VideoOne,
  CheckOne,
  Error,
  Time,
  LoadingOne,
  FullScreen,
  PlayOne,
  PauseOne,
} from '@icon-park/react';
import { useCloudAuth } from '@renderer/hooks/context/CloudAuthContext';
import {
  getGenerationTask,
  type GenerationTaskView,
  canvasMediaUrl,
} from '../videoCanvas/api';
import { rememberVideoGenerationTask } from './routeMemory';
import styles from './ClipResultPage.module.css';

const POLL_INTERVAL_MS = 2000;
const MAX_POLL_ATTEMPTS = 300; // 10 minutes max

type LocationState = {
  title?: string;
  prompt?: string;
  taskId?: string;
};

interface StatusConfig {
  icon: React.ReactNode;
  titleKey: string;
  titleDefault: string;
  subtitleKey: string;
  subtitleDefault: string;
  variant: 'default' | 'success' | 'error' | 'running';
}

const STATUS_CONFIGS: Record<string, StatusConfig> = {
  queued: {
    icon: <Time theme='outline' size={48} />,
    titleKey: 'videoGeneration.clip.status.queued',
    titleDefault: '排队中',
    subtitleKey: 'videoGeneration.clip.status.queuedDesc',
    subtitleDefault: '正在等待服务器处理...',
    variant: 'default',
  },
  running: {
    icon: <LoadingOne theme='outline' size={48} className={styles.spinning} />,
    titleKey: 'videoGeneration.clip.status.running',
    titleDefault: '生成中',
    subtitleKey: 'videoGeneration.clip.status.runningDesc',
    subtitleDefault: '正在渲染你的视频，请稍候...',
    variant: 'running',
  },
  succeeded: {
    icon: <CheckOne theme='outline' size={48} />,
    titleKey: 'videoGeneration.clip.status.succeeded',
    titleDefault: '生成完成',
    subtitleKey: 'videoGeneration.clip.status.succeededDesc',
    subtitleDefault: '视频已生成，可以下载或重新生成',
    variant: 'success',
  },
  failed: {
    icon: <Error theme='outline' size={48} />,
    titleKey: 'videoGeneration.clip.status.failed',
    titleDefault: '生成失败',
    subtitleKey: 'videoGeneration.clip.status.failedDesc',
    subtitleDefault: '处理过程中遇到问题',
    variant: 'error',
  },
  canceled: {
    icon: <Error theme='outline' size={48} />,
    titleKey: 'videoGeneration.clip.status.canceled',
    titleDefault: '已取消',
    subtitleKey: 'videoGeneration.clip.status.canceledDesc',
    subtitleDefault: '任务已被取消',
    variant: 'error',
  },
};

interface ProgressStep {
  key: string;
  labelKey: string;
  labelDefault: string;
  estimate: number; // seconds
}

const GENERATION_STEPS: ProgressStep[] = [
  { key: 'queued', labelKey: 'videoGeneration.clip.step.queued', labelDefault: '排队中', estimate: 5 },
  { key: 'initializing', labelKey: 'videoGeneration.clip.step.initializing', labelDefault: '初始化', estimate: 10 },
  { key: 'processing', labelKey: 'videoGeneration.clip.step.processing', labelDefault: '处理中', estimate: 30 },
  { key: 'rendering', labelKey: 'videoGeneration.clip.step.rendering', labelDefault: '渲染中', estimate: 45 },
  { key: 'finalizing', labelKey: 'videoGeneration.clip.step.finalizing', labelDefault: '完成中', estimate: 10 },
];

const ClipResultPage: React.FC = () => {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const location = useLocation();
  const { taskId: urlTaskId } = useParams<{ taskId: string }>();
  const { logout } = useCloudAuth();

  const taskId = urlTaskId || (location.state as LocationState)?.taskId;
  const title = (location.state as LocationState)?.title;
  const prompt = (location.state as LocationState)?.prompt;

  const [task, setTask] = useState<GenerationTaskView | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [videoUrl, setVideoUrl] = useState<string | null>(null);
  const [elapsedSeconds, setElapsedSeconds] = useState(0);
  const [isPlaying, setIsPlaying] = useState(false);
  const [isFullscreen, setIsFullscreen] = useState(false);

  const pollAttemptsRef = useRef(0);
  const videoRef = useRef<HTMLVideoElement>(null);
  const startTimeRef = useRef<number | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  // Calculate estimated progress step based on elapsed time
  const currentStep = useMemo(() => {
    if (!task || task.status !== 'running') return null;
    const elapsed = elapsedSeconds;
    let accumulated = 0;
    for (const step of GENERATION_STEPS) {
      accumulated += step.estimate;
      if (elapsed < accumulated) {
        return step;
      }
    }
    return GENERATION_STEPS[GENERATION_STEPS.length - 1];
  }, [task, elapsedSeconds]);

  // Estimate progress percentage
  const estimatedProgress = useMemo(() => {
    if (!task) return 0;
    if (task.status === 'queued') return 5;
    if (task.status === 'succeeded') return 100;
    if (task.progress != null) return Math.round(task.progress);
    if (task.status === 'running' && currentStep) {
      const stepIndex = GENERATION_STEPS.findIndex((s) => s.key === currentStep.key);
      const baseProgress = (stepIndex / GENERATION_STEPS.length) * 100;
      const stepProgress = (elapsedSeconds / (currentStep.estimate * GENERATION_STEPS.length)) * (100 / GENERATION_STEPS.length);
      return Math.min(Math.round(baseProgress + stepProgress), 95);
    }
    return 0;
  }, [task, currentStep, elapsedSeconds]);

  const fetchTask = useCallback(async () => {
    if (!taskId) {
      setError(t('videoGeneration.clip.taskNotFound'));
      return true;
    }
    try {
      const result = await getGenerationTask(taskId);
      setTask(result);
      setError(null);

      // Track this clip task in the MRU strip so it appears under the sider
      // entry and survives page reloads — mirrors the create-time call.
      const resolvedTitle = title?.trim() || result.prompt?.trim().slice(0, 48);
      rememberVideoGenerationTask(taskId, resolvedTitle);

      if (result.status === 'succeeded') {
        if (result.result_media_id) {
          setVideoUrl(canvasMediaUrl(result.result_media_id));
        }
        return true;
      }

      if (result.status === 'failed') {
        setError(result.error || t('videoGeneration.clip.generationFailed'));
        return true;
      }

      if (result.status === 'canceled') {
        setError(t('videoGeneration.clip.generationCanceled'));
        return true;
      }

      return false;
    } catch (e: unknown) {
      const err = e as { status?: number; code?: string; message?: string };
      if (err?.status === 401 || (err?.status === 400 && /session|token|credential|authentication/i.test(err?.code || ''))) {
        await logout();
        navigate('/cloud-login');
        return true;
      }
      const errorMessage = (e as Error).message ?? String(e);
      console.error('[ClipResultPage] failed to fetch task:', errorMessage);
      setError(errorMessage);
      return true;
    }
  }, [taskId, logout, navigate, t]);

  // Start timer when task starts running
  useEffect(() => {
    if (task?.status === 'running' && startTimeRef.current === null) {
      startTimeRef.current = Date.now();
    }
    if (task?.status === 'queued') {
      startTimeRef.current = null;
    }
  }, [task?.status]);

  // Update elapsed time every second while running
  useEffect(() => {
    if (task?.status !== 'running' && task?.status !== 'queued') {
      return;
    }

    const interval = setInterval(() => {
      if (startTimeRef.current !== null) {
        setElapsedSeconds(Math.floor((Date.now() - startTimeRef.current) / 1000));
      }
    }, 1000);

    return () => clearInterval(interval);
  }, [task?.status]);

  useEffect(() => {
    let pollInterval: ReturnType<typeof setInterval> | null = null;

    const startPolling = async () => {
      setLoading(true);
      const done = await fetchTask();
      setLoading(false);

      if (!done) {
        pollInterval = setInterval(async () => {
          pollAttemptsRef.current += 1;
          if (pollAttemptsRef.current >= MAX_POLL_ATTEMPTS) {
            if (pollInterval) clearInterval(pollInterval);
            setError(t('videoGeneration.clip.generationTimeout'));
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

  useEffect(() => {
    return () => {
      if (videoUrl?.startsWith('blob:')) {
        URL.revokeObjectURL(videoUrl);
      }
    };
  }, [videoUrl]);

  const handleBack = useCallback(() => {
    navigate('/video-generation?mode=generate');
  }, [navigate]);

  const handleRetry = useCallback(() => {
    setError(null);
    setTask(null);
    setVideoUrl(null);
    setElapsedSeconds(0);
    startTimeRef.current = null;
    pollAttemptsRef.current = 0;
    void fetchTask();
  }, [fetchTask]);

  const handleDownload = useCallback(() => {
    if (!videoUrl || !task) return;
    const a = document.createElement('a');
    a.href = videoUrl;
    a.download = `${title || t('videoGeneration.clip.defaultTitle')}.mp4`;
    a.click();
  }, [videoUrl, title, t]);

  const handlePlayPause = useCallback(() => {
    const video = videoRef.current;
    if (!video) return;
    if (isPlaying) {
      video.pause();
    } else {
      void video.play();
    }
    setIsPlaying(!isPlaying);
  }, [isPlaying]);

  const handleFullscreen = useCallback(() => {
    if (!containerRef.current) return;
    if (isFullscreen) {
      document.exitFullscreen?.();
    } else {
      void containerRef.current.requestFullscreen?.();
    }
    setIsFullscreen(!isFullscreen);
  }, [isFullscreen]);

  useEffect(() => {
    const handleFullscreenChange = () => {
      setIsFullscreen(!!document.fullscreenElement);
    };
    document.addEventListener('fullscreenchange', handleFullscreenChange);
    return () => document.removeEventListener('fullscreenchange', handleFullscreenChange);
  }, []);

  const statusConfig = task ? (STATUS_CONFIGS[task.status] || STATUS_CONFIGS.queued) : STATUS_CONFIGS.queued;

  const isRunning = task?.status === 'queued' || task?.status === 'running';
  const isSucceeded = task?.status === 'succeeded';
  const isFailed = task?.status === 'failed' || task?.status === 'canceled';

  // Format elapsed time
  const formatElapsed = (seconds: number) => {
    if (seconds < 60) return `${seconds}s`;
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    return `${mins}m ${secs}s`;
  };

  return (
    <div ref={containerRef} className={`${styles.page} page`}>
      {/* Header */}
      <div className={styles.header}>
        <Button
          shape='round'
          type='outline'
          size='small'
          onClick={handleBack}
          className={styles.backBtn}
        >
          <span className={styles.btnIcon}>
            <ArrowLeft theme='outline' size={14} />
          </span>
          {t('videoGeneration.clip.back')}
        </Button>
        <div className={styles.titleSection}>
          <h1 className={styles.title}>
            {title || t('videoGeneration.clip.defaultTitle')}
          </h1>
          {prompt && (
            <p className={styles.prompt}>{prompt}</p>
          )}
        </div>
        {taskId && (
          <div className={styles.taskIdBadge}>
            <span className={styles.taskIdLabel}>ID:</span>
            <span className={styles.taskIdValue}>{taskId.slice(0, 8)}...</span>
          </div>
        )}
      </div>

      {/* Main Content */}
      <div className={styles.content}>
        {loading && !task && (
          <div className={styles.loadingState}>
            <div className={styles.loadingPulse}>
              <Spin size={40} />
            </div>
            <p className={styles.loadingText}>
              {t('videoGeneration.clip.loading')}
            </p>
            <p className={styles.loadingHint}>
              {t('videoGeneration.clip.loadingHint', { defaultValue: '正在连接到服务器...' })}
            </p>
          </div>
        )}

        {error && !task && (
          <div className={styles.errorState}>
            <Result
              status='error'
              title={t('videoGeneration.clip.errorTitle')}
              subTitle={error}
            />
            <div className={styles.errorActions}>
              <Button
                shape='round'
                type='outline'
                onClick={handleBack}
              >
                <span className={styles.btnIcon}>
                  <ArrowLeft theme='outline' size={14} />
                </span>
                {t('videoGeneration.clip.back')}
              </Button>
              <Button
                shape='round'
                type='primary'
                onClick={handleRetry}
              >
                <span className={styles.btnIcon}>
                  <Refresh theme='outline' size={14} />
                </span>
                {t('videoGeneration.clip.retry')}
              </Button>
            </div>
          </div>
        )}

        {task && (
          <>
            {/* Progress / Status Card */}
            <div className={`${styles.statusCard} ${styles[`statusCard${capitalize(statusConfig.variant)}`]}`}>
              <div className={styles.statusIconWrapper}>
                <div className={`${styles.statusIcon} ${isRunning ? styles.statusIconPulse : ''}`}>
                  {statusConfig.icon}
                </div>
              </div>
              <div className={styles.statusInfo}>
                <h2 className={styles.statusTitle}>
                  {t(statusConfig.titleKey, { defaultValue: statusConfig.titleDefault })}
                </h2>
                <p className={styles.statusSubtitle}>
                  {t(statusConfig.subtitleKey, { defaultValue: statusConfig.subtitleDefault })}
                </p>

                {/* Progress Bar for running state */}
                {(isRunning) && (
                  <div className={styles.progressSection}>
                    <Progress
                      percent={estimatedProgress}
                      showText={false}
                      className={styles.progressBar}
                      strokeWidth={6}
                    />
                    <div className={styles.progressLabels}>
                      <span className={styles.progressPercent}>{estimatedProgress}%</span>
                      {isRunning && (
                        <span className={styles.progressTime}>
                          {t('videoGeneration.clip.elapsed', { time: formatElapsed(elapsedSeconds) })}
                        </span>
                      )}
                    </div>
                  </div>
                )}

                {/* Current step indicator */}
                {isRunning && currentStep && (
                  <div className={styles.stepIndicator}>
                    <span className={styles.stepDot} />
                    <span className={styles.stepText}>
                      {t(currentStep.labelKey, { defaultValue: currentStep.labelDefault })}
                    </span>
                  </div>
                )}
              </div>
            </div>

            {/* Video Player */}
            {isSucceeded && videoUrl && (
              <div className={styles.videoContainer}>
                <div className={styles.videoWrapper}>
                  <video
                    ref={videoRef}
                    className={styles.videoPlayer}
                    src={videoUrl}
                    controls={false}
                    playsInline
                    onPlay={() => setIsPlaying(true)}
                    onPause={() => setIsPlaying(false)}
                    onEnded={() => setIsPlaying(false)}
                  />
                  <div className={styles.videoOverlay}>
                    <button
                      className={styles.videoControlBtn}
                      onClick={handlePlayPause}
                      aria-label={isPlaying ? '暂停' : '播放'}
                    >
                      {isPlaying ? <PauseOne theme='outline' size={24} /> : <PlayOne theme='outline' size={24} />}
                    </button>
                    <button
                      className={styles.videoControlBtn}
                      onClick={handleFullscreen}
                      aria-label='全屏'
                    >
                      <FullScreen theme='outline' size={24} />
                    </button>
                  </div>
                </div>
              </div>
            )}

            {/* Error Banner */}
            {isFailed && error && (
              <div className={styles.errorBanner}>
                <div className={styles.errorBannerContent}>
                  <Error theme='outline' size={20} className={styles.errorBannerIcon} />
                  <span>{error}</span>
                </div>
              </div>
            )}

            {/* Actions */}
            <div className={styles.actions}>
              {isSucceeded && (
                <Button
                  shape='round'
                  type='primary'
                  size='large'
                  onClick={handleDownload}
                  className={styles.actionBtn}
                >
                  <span className={styles.btnIcon}>
                    <Download theme='outline' size={16} />
                  </span>
                  <span className={styles.btnText}>{t('videoGeneration.clip.download')}</span>
                </Button>
              )}
              <Button
                shape='round'
                type='outline'
                size='large'
                onClick={handleRetry}
                className={styles.actionBtn}
              >
                <span className={styles.btnIcon}>
                  <Refresh theme='outline' size={16} />
                </span>
                <span className={styles.btnText}>
                  {isSucceeded ? t('videoGeneration.clip.regenerate') : t('videoGeneration.clip.refresh')}
                </span>
              </Button>
            </div>
          </>
        )}
      </div>
    </div>
  );
};

// Helper function
function capitalize(str: string): string {
  return str.charAt(0).toUpperCase() + str.slice(1);
}

export default ClipResultPage;
