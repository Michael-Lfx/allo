import React, { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Spin } from '@arco-design/web-react';
import { FullScreen, Left, LoadingFour, Music, Right, VideoOne } from '@icon-park/react';
import { getArtifact } from '../api';
import { seekMediaElementToFirstFrame } from '../mediaFirstFrame';
import { useArtifactMediaUrl } from '../useArtifactMediaUrl';
import {
  buildStoryboardScenesFromStoryboards,
  findStoryboardPaths,
  mergeStoryboardsWithoutGrowth,
  parseStoryboard,
  storyboardRefreshSignature,
  type StoryboardScene,
  type StoryboardShot,
} from '../artifactPresentation';
import type { ShotCopySaveResult } from '../storyboardShotCopy';
import StoryboardShotEditorModal, {
  type StoryboardShotEditorFocus,
} from './StoryboardShotEditorModal';
import {
  activeVideoGenerationTarget,
  resolveStoryboardVideoStatus,
  type StoryboardVideoSlotStatus,
} from '../storyboardVideoStatus';
import type { ArtifactNode } from '../types';
import { useRunStatusFull } from '../useRunStatusFeed';
import styles from '../index.module.css';

const InspectorSpecBlock: React.FC<{
  label: string;
  body: string;
}> = ({ label, body }) => (
  <div className={styles.storyInspectorSpecBlock}>
    <div className={styles.storyInspectorSubLabel}>{label}</div>
    <p className={`${styles.storyInspectorBody} text-13px leading-21px text-white/90`}>{body}</p>
  </div>
);

function activateInspectorSection(
  event: React.KeyboardEvent<HTMLDivElement>,
  open: () => void
): void {
  if (event.key === 'Enter' || event.key === ' ') {
    event.preventDefault();
    open();
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
}) => {
  const { t } = useTranslation();
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
  const [editorFocus, setEditorFocus] = useState<StoryboardShotEditorFocus | null>(null);

  const generatingTarget = useMemo(
    () => activeVideoGenerationTarget(runStatus),
    [runStatus]
  );
  const rendering = runStatus?.status === 'rendering';

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

  const selectScene = useCallback(
    (sceneId: string) => {
      syncedFocusSceneIdRef.current = sceneId;
      setActiveSceneId(sceneId);
      onFocusScene?.(sceneId);
    },
    [onFocusScene]
  );

  const applySavedCopy = useCallback((result: ShotCopySaveResult) => {
    setStoryboardEntries((previous) =>
      previous.map((entry) =>
        entry.path === result.storyboardPath
          ? { path: entry.path, shots: parseStoryboard(result.patchedText) }
          : entry
      )
    );
  }, []);

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
  const activeSceneIndex = scenes.findIndex((scene) => scene.id === activeScene.id);
  const expandLabel = t('videoGeneration.studio.storyboard.expand', {
    defaultValue: '展开编辑',
  });

  return (
    <div className={styles.storyboardLayout}>
      <div className={styles.storyStage}>
        <div className={styles.storyMedia}>
          <SceneMedia
            key={activeScene.id}
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
            {t('videoGeneration.studio.storyboard.shotNumberOf', {
              number: sceneNumber,
              total: scenes.length,
              defaultValue: '镜头 {{number}} / {{total}}',
            })}
            {activeScene.beatCount != null
              ? ` · ${t('videoGeneration.studio.storyboard.packedBeats', {
                  count: activeScene.beatCount,
                  defaultValue: '{{count}} 个切镜一次生成',
                })}`
              : ''}
          </span>
        </div>
        <aside className={styles.storyInspector}>
          <div
            className={`${styles.storyInspectorOpen} ${styles.storyInspectorVisual}`}
            role='button'
            tabIndex={0}
            aria-label={`${t('videoGeneration.studio.storyboard.visualDirection', {
              defaultValue: '画面描述',
            })} · ${expandLabel}`}
            onClick={() => setEditorFocus('visual')}
            onKeyDown={(event) =>
              activateInspectorSection(event, () => setEditorFocus('visual'))
            }
          >
            <div className={styles.storyInspectorLabelRow}>
              <div className={styles.storyInspectorLabel}>
                {t('videoGeneration.studio.storyboard.visualDirection', {
                  defaultValue: '画面描述',
                })}
              </div>
              <FullScreen
                theme='outline'
                size={14}
                className={styles.storyInspectorExpand}
              />
            </div>
            {activeScene.beatCount != null ? (
              <p className='m-0 text-12px leading-18px text-white/55'>
                {t('videoGeneration.studio.storyboard.packedBeatsHint', {
                  count: activeScene.beatCount,
                  defaultValue: '相邻短镜头已合并进这一条成片，生成时只出一条视频。',
                })}
              </p>
            ) : null}
            <div className={styles.storyInspectorScroll}>
              <p className={`${styles.storyInspectorBody} text-14px leading-23px text-white/90`}>
                {activeScene.visualDescription ||
                  t('videoGeneration.studio.storyboard.visualPending', {
                    defaultValue: '画面生成后将在这里展示。',
                  })}
              </p>
              {activeScene.beats && activeScene.beats.length >= 2 ? (
                <div className={`${styles.storyInspectorSpecStack} mt-12px`}>
                  {activeScene.beats.map((beat, beatIndex) => (
                    <InspectorSpecBlock
                      key={`${activeScene.id}-beat-${beatIndex}`}
                      label={t('videoGeneration.studio.storyboard.packedBeatItem', {
                        number: beatIndex + 1,
                        defaultValue: '切镜 {{number}}',
                      })}
                      body={beat.visualDescription}
                    />
                  ))}
                </div>
              ) : null}
            </div>
          </div>
          <div className='shrink-0 border-t border-white/10' />
          <div
            className={`${styles.storyInspectorOpen} ${styles.storyInspectorAudio}`}
            role='button'
            tabIndex={0}
            aria-label={`${t('videoGeneration.studio.storyboard.audioDirection', {
              defaultValue: '音频 / 台词',
            })} · ${expandLabel}`}
            onClick={() => setEditorFocus('audio')}
            onKeyDown={(event) =>
              activateInspectorSection(event, () => setEditorFocus('audio'))
            }
          >
            <div className={styles.storyInspectorLabelRow}>
              <div className={styles.storyInspectorLabel}>
                {t('videoGeneration.studio.storyboard.audioDirection', {
                  defaultValue: '音频 / 台词',
                })}
              </div>
              <FullScreen
                theme='outline'
                size={14}
                className={styles.storyInspectorExpand}
              />
            </div>
            <div className={styles.storyInspectorScroll}>
              <div className='flex items-start gap-7px text-12px leading-18px text-white/58'>
                <Music theme='outline' size={14} className='mt-2px shrink-0' />
                <span className={styles.storyInspectorBody}>
                  {activeScene.audioDescription ||
                    t('videoGeneration.studio.storyboard.audioPending', {
                      defaultValue: '暂无音频或台词描述',
                    })}
                </span>
              </div>
            </div>
          </div>
        </aside>
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
            // Compact thumbs prefer the last-frame still so a ready shot never
            // downloads its whole clip just to render a thumbnail.
            const thumbPath = scene.imagePath ?? scene.videoPath;
            return (
              <div key={scene.id} className={styles.shotCardStack} data-scene-id={scene.id}>
              <button
                type='button'
                className={`${styles.shotCard} ${active ? styles.shotCardActive : ''} ${
                  status === 'generating' ? styles.shotCardGenerating : ''
                }`}
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
                {scene.beatCount != null ? (
                  <span className={styles.shotPackedBadge}>
                    {t('videoGeneration.studio.storyboard.packedBeatsShort', {
                      count: scene.beatCount,
                      defaultValue: '{{count}} 切',
                    })}
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
                        {beat.visualDescription
                          ? ` · ${beat.visualDescription}`
                          : ''}
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
      {editorFocus ? (
        <StoryboardShotEditorModal
          sessionId={sessionId}
          scene={activeScene}
          sceneNumber={sceneNumber}
          total={scenes.length}
          visible
          focusField={editorFocus}
          hasPrev={activeSceneIndex > 0}
          hasNext={activeSceneIndex >= 0 && activeSceneIndex < scenes.length - 1}
          onClose={() => setEditorFocus(null)}
          onPrev={() => {
            const previous = scenes[activeSceneIndex - 1];
            if (previous) selectScene(previous.id);
          }}
          onNext={() => {
            const next = scenes[activeSceneIndex + 1];
            if (next) selectScene(next.id);
          }}
          onSaved={applySavedCopy}
        />
      ) : null}
    </div>
  );
};

export default StoryboardBoard;
