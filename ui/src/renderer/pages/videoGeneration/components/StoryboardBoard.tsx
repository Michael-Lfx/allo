import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, Input, Spin } from '@arco-design/web-react';
import { Edit, LoadingFour, Music, VideoOne } from '@icon-park/react';
import { getArtifact, loadArtifactMediaUrl } from '../api';
import {
  buildStoryboardScenesFromStoryboards,
  findStoryboardPaths,
  parseStoryboard,
  type StoryboardScene,
  type StoryboardShot,
} from '../artifactPresentation';
import {
  activeVideoGenerationTarget,
  resolveStoryboardVideoStatus,
  type StoryboardVideoSlotStatus,
} from '../storyboardVideoStatus';
import type { ArtifactNode, SessionStatus } from '../types';
import styles from '../index.module.css';

const TextArea = Input.TextArea;

interface StoryboardBoardProps {
  sessionId: string;
  artifacts: ArtifactNode[];
  /** Live pipeline status — drives per-shot Video pending / Generating badges. */
  runStatus?: SessionStatus | null;
  disabled?: boolean;
  revising?: boolean;
  /** Persist edited Visual / audio direction for the active shot. */
  onSaveSceneDescriptions: (
    scene: StoryboardScene,
    descriptions: { visualDescription: string; audioDescription: string }
  ) => Promise<void> | void;
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
                defaultValue: 'Shot videos appear here after film generation',
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
  const [url, setUrl] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    if (!path) {
      setUrl(null);
      setFailed(false);
      return;
    }
    let cancelled = false;
    setFailed(false);
    void loadArtifactMediaUrl(sessionId, path)
      .then((nextUrl) => {
        if (cancelled) {
          URL.revokeObjectURL(nextUrl);
          return;
        }
        setUrl((previous) => {
          if (previous?.startsWith('blob:')) URL.revokeObjectURL(previous);
          return nextUrl;
        });
      })
      .catch(() => {
        if (!cancelled) setFailed(true);
      });
    return () => {
      cancelled = true;
    };
  }, [path, sessionId]);

  useEffect(
    () => () => {
      if (url?.startsWith('blob:')) URL.revokeObjectURL(url);
    },
    [url]
  );

  // Ready clip — show the video player / filmstrip preview.
  if (video && path) {
    if (failed) {
      return <VideoOne theme='outline' size={compact ? 20 : 34} className='opacity-35' />;
    }
    if (!url) return <Spin size={compact ? 12 : 18} />;
    return (
      <video
        src={url}
        controls={!compact}
        muted={compact}
        playsInline
        preload={compact ? 'metadata' : 'auto'}
        className={compact ? 'h-full w-full object-contain' : styles.storyShot}
      />
    );
  }

  // Pending / generating — optional still underneath a high-contrast status chip.
  const status: Exclude<StoryboardVideoSlotStatus, 'ready'> =
    videoStatus === 'generating' ? 'generating' : 'pending';

  if (path && url && !failed) {
    return (
      <div className={styles.videoStatusMediaWrap}>
        <img
          src={url}
          alt={alt}
          className={compact ? 'h-full w-full object-cover opacity-50' : `${styles.storyShot} opacity-50`}
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
  runStatus,
  disabled,
  revising,
  onSaveSceneDescriptions,
}) => {
  const { t } = useTranslation();
  const storyboardPaths = useMemo(() => findStoryboardPaths(artifacts), [artifacts]);
  const storyboardPathSignature = storyboardPaths.join('|');
  const [storyboardEntries, setStoryboardEntries] = useState<
    Array<{ path: string; shots: StoryboardShot[] }>
  >([]);
  const [activeSceneId, setActiveSceneId] = useState<string>();
  const [editMode, setEditMode] = useState(false);
  const [visualDraft, setVisualDraft] = useState('');
  const [audioDraft, setAudioDraft] = useState('');

  const generatingTarget = useMemo(
    () => activeVideoGenerationTarget(runStatus),
    [runStatus]
  );
  const rendering = runStatus?.status === 'rendering';

  useEffect(() => {
    if (!storyboardPaths.length) {
      setStoryboardEntries([]);
      return;
    }
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
    ).then((entries) => {
      if (!cancelled) setStoryboardEntries(entries);
    });
    return () => {
      cancelled = true;
    };
    // 轮询期间 artifacts 数组每 ~4s 换新身份；以稳定签名为依赖，路径集合未变就不重拉全部分镜。
    // eslint-disable-next-line react-hooks/exhaustive-deps -- effect 体读取的 storyboardPaths 内容由上方签名完全决定
  }, [sessionId, storyboardPathSignature]);

  const scenes = useMemo(
    () => buildStoryboardScenesFromStoryboards(artifacts, storyboardEntries),
    [artifacts, storyboardEntries]
  );
  const activeScene =
    scenes.find((scene) => scene.id === activeSceneId) ??
    scenes[0];

  useEffect(() => {
    setEditMode(false);
    setVisualDraft(activeScene?.visualDescription ?? '');
    setAudioDraft(activeScene?.audioDescription ?? '');
  }, [activeScene?.id, activeScene?.visualDescription, activeScene?.audioDescription]);

  const canEdit = Boolean(activeScene?.storyboardPath || activeScene?.revisionPath);

  const startEdit = useCallback(() => {
    if (!activeScene) return;
    setVisualDraft(activeScene.visualDescription || '');
    setAudioDraft(activeScene.audioDescription || '');
    setEditMode(true);
  }, [activeScene]);

  const cancelEdit = useCallback(() => {
    setVisualDraft(activeScene?.visualDescription ?? '');
    setAudioDraft(activeScene?.audioDescription ?? '');
    setEditMode(false);
  }, [activeScene?.audioDescription, activeScene?.visualDescription]);

  const handleSave = useCallback(async () => {
    if (!activeScene || !visualDraft.trim()) return;
    const nextVisual = visualDraft.trim();
    const nextAudio = audioDraft.trim();
    try {
      await onSaveSceneDescriptions(activeScene, {
        visualDescription: nextVisual,
        audioDescription: nextAudio,
      });
      setStoryboardEntries((previous) =>
        previous.map((entry) => {
          if (entry.path !== activeScene.storyboardPath) return entry;
          return {
            ...entry,
            shots: entry.shots.map((shot) =>
              shot.index === activeScene.shotIndex
                ? {
                    ...shot,
                    visualDescription: nextVisual,
                    audioDescription: nextAudio || undefined,
                  }
                : shot
            ),
          };
        })
      );
      setEditMode(false);
    } catch {
      // Parent already surfaces the error toast; keep the editor open.
    }
  }, [activeScene, audioDraft, onSaveSceneDescriptions, visualDraft]);

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

  if (!activeScene) {
    return (
      <div className='flex min-h-240px flex-col items-center justify-center gap-8px rd-14px border border-dashed border-[var(--color-border-2)] text-center'>
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

  return (
    <div className='flex flex-col gap-12px'>
      <div className={styles.storyStage}>
        <div className={styles.storyMedia}>
          <SceneMedia
            sessionId={sessionId}
            path={mainPath}
            video={mainIsVideo}
            alt={t('videoGeneration.studio.storyboard.shotAlt', {
              number: sceneNumber,
              defaultValue: '镜头 {{number}}',
            })}
            videoStatus={activeVideoStatus}
          />
          <span className='absolute left-14px top-14px z-2 rd-full bg-black/55 px-9px py-4px text-11px font-650 text-white backdrop-blur'>
            {t('videoGeneration.studio.storyboard.shotNumber', {
              number: sceneNumber,
              defaultValue: '镜头 {{number}}',
            })}
          </span>
        </div>
        <aside className={styles.storyInspector}>
          <div className={styles.storyInspectorLabel}>
            {t('videoGeneration.studio.storyboard.visualDirection', {
              defaultValue: '画面描述',
            })}
          </div>

          {editMode ? (
            <>
              <div className={`${styles.storyInspectorScroll} ${styles.storyInspectorVisual}`}>
                <TextArea
                  value={visualDraft}
                  onChange={setVisualDraft}
                  disabled={disabled || revising}
                  autoSize={{ minRows: 4, maxRows: 12 }}
                  className={styles.reviseInlineInput}
                  style={{
                    color: 'rgba(255,255,255,0.92)',
                    WebkitTextFillColor: 'rgba(255,255,255,0.92)',
                    caretColor: 'rgba(255,255,255,0.92)',
                    background: 'rgba(255,255,255,0.08)',
                  }}
                  placeholder={t('videoGeneration.studio.storyboard.visualEditPlaceholder', {
                    defaultValue: '描述这个镜头的画面…',
                  })}
                />
              </div>
              <div className={styles.storyInspectorLabel}>
                {t('videoGeneration.studio.storyboard.audioDirection', {
                  defaultValue: '音频 / 台词',
                })}
              </div>
              <div className={`${styles.storyInspectorScroll} ${styles.storyInspectorAudio}`}>
                <TextArea
                  value={audioDraft}
                  onChange={setAudioDraft}
                  disabled={disabled || revising}
                  autoSize={{ minRows: 2, maxRows: 10 }}
                  className={styles.reviseInlineInput}
                  style={{
                    color: 'rgba(255,255,255,0.92)',
                    WebkitTextFillColor: 'rgba(255,255,255,0.92)',
                    caretColor: 'rgba(255,255,255,0.92)',
                    background: 'rgba(255,255,255,0.08)',
                  }}
                  placeholder={t('videoGeneration.studio.storyboard.audioEditPlaceholder', {
                    defaultValue: '背景音乐、环境音或台词…',
                  })}
                />
              </div>
              <div className={`${styles.storyInspectorActions} flex flex-wrap items-center gap-8px`}>
                <Button
                  size='small'
                  className='!border-white/15 !bg-transparent !text-white/80 hover:!bg-white/10'
                  disabled={revising}
                  onClick={cancelEdit}
                >
                  {t('common.cancel', { defaultValue: '取消' })}
                </Button>
                <Button
                  type='primary'
                  size='small'
                  loading={revising}
                  disabled={disabled || !visualDraft.trim()}
                  onClick={() => void handleSave()}
                >
                  {t('videoGeneration.artifacts.save', { defaultValue: '保存' })}
                </Button>
              </div>
            </>
          ) : (
            <>
              <div className={`${styles.storyInspectorScroll} ${styles.storyInspectorVisual}`}>
                <p className='m-0 text-14px leading-23px text-white/90'>
                  {activeScene.visualDescription ||
                    t('videoGeneration.studio.storyboard.visualPending', {
                      defaultValue: '画面生成后将在这里展示。',
                    })}
                </p>
              </div>
              <div className='shrink-0 border-t border-white/10' />
              <div className={styles.storyInspectorLabel}>
                {t('videoGeneration.studio.storyboard.audioDirection', {
                  defaultValue: '音频 / 台词',
                })}
              </div>
              <div className={`${styles.storyInspectorScroll} ${styles.storyInspectorAudio}`}>
                <div className='flex items-start gap-7px text-12px leading-18px text-white/58'>
                  <Music theme='outline' size={14} className='mt-2px shrink-0' />
                  <span>
                    {activeScene.audioDescription ||
                      t('videoGeneration.studio.storyboard.audioPending', {
                        defaultValue: '暂无音频或台词描述',
                      })}
                  </span>
                </div>
              </div>
              <Button
                className={`${styles.storyInspectorActions} !border-white/15 !bg-white/8 !text-white hover:!bg-white/14`}
                disabled={disabled || !canEdit}
                onClick={startEdit}
              >
                <span className='inline-flex items-center gap-6px'>
                  <Edit theme='outline' size={14} />
                  {t('videoGeneration.studio.storyboard.reviseShot', {
                    defaultValue: '修改这个镜头',
                  })}
                </span>
              </Button>
            </>
          )}
        </aside>
      </div>

      <div className={styles.filmstrip} aria-label={t('videoGeneration.studio.storyboard.filmstrip', { defaultValue: '分镜胶片' })}>
        {scenes.map((scene) => {
          const number = scene.index + 1;
          const active = scene.id === activeScene.id;
          const status = videoStatusFor(scene);
          const thumbPath =
            scene.videoPath ?? (status === 'ready' ? undefined : scene.imagePath);
          return (
            <button
              key={scene.id}
              type='button'
              className={`${styles.shotCard} ${active ? styles.shotCardActive : ''} ${
                status === 'generating' ? styles.shotCardGenerating : ''
              }`}
              aria-pressed={active}
              onClick={() => setActiveSceneId(scene.id)}
            >
              <span className={styles.shotThumb}>
                <SceneMedia
                  sessionId={sessionId}
                  path={thumbPath}
                  video={Boolean(scene.videoPath)}
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
              </span>
              <span className='block truncate px-9px py-8px text-11px'>
                {scene.visualDescription ||
                  t('videoGeneration.studio.storyboard.shotNumber', {
                    number,
                    defaultValue: '镜头 {{number}}',
                  })}
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
};

export default StoryboardBoard;
