
import React, { useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, Input, Spin } from '@arco-design/web-react';
import { Edit, Music, VideoOne } from '@icon-park/react';
import { getArtifact, loadArtifactMediaUrl } from '../api';
import {
  buildStoryboardScenesFromStoryboards,
  findStoryboardPaths,
  parseStoryboard,
  type StoryboardScene,
  type StoryboardShot,
} from '../artifactPresentation';
import type { ArtifactNode } from '../types';
import styles from '../index.module.css';

const TextArea = Input.TextArea;

interface StoryboardBoardProps {
  sessionId: string;
  artifacts: ArtifactNode[];
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
}

const SceneMedia: React.FC<SceneMediaProps> = ({ sessionId, path, video, compact, alt }) => {
  const { t } = useTranslation();
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

  if (!path) {
    return (
      <div
        className={`flex flex-col items-center justify-center text-center ${
          compact ? 'gap-4px px-8px' : 'gap-8px px-24px'
        }`}
        role='status'
      >
        <VideoOne
          theme='outline'
          size={compact ? 18 : 30}
          className='text-white/35'
        />
        <div
          className={`font-600 text-white/72 ${compact ? 'text-10px leading-14px' : 'text-13px'}`}
        >
          {t('videoGeneration.studio.storyboard.videoPending', {
            defaultValue: 'Video pending',
          })}
        </div>
        {compact ? null : (
          <div className='max-w-320px text-12px leading-18px text-white/45'>
            {t('videoGeneration.studio.storyboard.videoPendingHint', {
              defaultValue: 'Shot videos appear here after film generation',
            })}
          </div>
        )}
      </div>
    );
  }
  if (failed) {
    return <VideoOne theme='outline' size={compact ? 20 : 34} className='opacity-35' />;
  }
  if (!url) return <Spin size={compact ? 12 : 18} />;
  if (video) {
    return (
      <video
        src={url}
        controls={!compact}
        muted={compact}
        playsInline
        preload={compact ? 'metadata' : 'auto'}
        className='h-full w-full object-contain'
      />
    );
  }
  return <img src={url} alt={alt} className='h-full w-full object-cover' />;
};

const StoryboardBoard: React.FC<StoryboardBoardProps> = ({
  sessionId,
  artifacts,
  disabled,
  revising,
  onSaveSceneDescriptions,
}) => {
  const { t } = useTranslation();
  const storyboardPaths = useMemo(() => findStoryboardPaths(artifacts), [artifacts]);
  const [storyboardEntries, setStoryboardEntries] = useState<
    Array<{ path: string; shots: StoryboardShot[] }>
  >([]);
  const [activeSceneId, setActiveSceneId] = useState<string>();
  const [editMode, setEditMode] = useState(false);
  const [visualDraft, setVisualDraft] = useState('');
  const [audioDraft, setAudioDraft] = useState('');

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
  }, [sessionId, storyboardPaths]);

  const scenes = useMemo(
    () => buildStoryboardScenesFromStoryboards(artifacts, storyboardEntries).slice(0, 60),
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

  const mainPath = activeScene.videoPath ?? activeScene.imagePath;
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
          />
          <span className='absolute left-14px top-14px rd-full bg-black/55 px-9px py-4px text-11px font-650 text-white backdrop-blur'>
            {t('videoGeneration.studio.storyboard.shotNumber', {
              number: sceneNumber,
              defaultValue: '镜头 {{number}}',
            })}
          </span>
        </div>
        <aside className={styles.storyInspector}>
          <div className='mb-7px text-10px font-700 uppercase tracking-[0.14em] text-white/45'>
            {t('videoGeneration.studio.storyboard.visualDirection', {
              defaultValue: '画面描述',
            })}
          </div>

          {editMode ? (
            <>
              <TextArea
                value={visualDraft}
                onChange={setVisualDraft}
                disabled={disabled || revising}
                autoSize={{ minRows: 4, maxRows: 8 }}
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
              <div className='mb-6px mt-14px text-10px font-700 uppercase tracking-[0.14em] text-white/45'>
                {t('videoGeneration.studio.storyboard.audioDirection', {
                  defaultValue: '音频 / 台词',
                })}
              </div>
              <TextArea
                value={audioDraft}
                onChange={setAudioDraft}
                disabled={disabled || revising}
                autoSize={{ minRows: 2, maxRows: 6 }}
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
              <div className='mt-10px flex flex-wrap items-center gap-8px'>
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
              <p className='m-0 text-14px leading-23px text-white/90'>
                {activeScene.visualDescription ||
                  t('videoGeneration.studio.storyboard.visualPending', {
                    defaultValue: '画面生成后将在这里展示。',
                  })}
              </p>
              <div className='mt-14px border-t border-white/10 pt-12px'>
                <div className='mb-6px text-10px font-700 uppercase tracking-[0.14em] text-white/45'>
                  {t('videoGeneration.studio.storyboard.audioDirection', {
                    defaultValue: '音频 / 台词',
                  })}
                </div>
                <div className='flex items-start gap-7px text-12px leading-18px text-white/58'>
                  <Music theme='outline' size={14} className='mt-2px shrink-0' />
                  {activeScene.audioDescription ||
                    t('videoGeneration.studio.storyboard.audioPending', {
                      defaultValue: '暂无音频或台词描述',
                    })}
                </div>
              </div>
              <Button
                className='!mt-18px !border-white/15 !bg-white/8 !text-white hover:!bg-white/14'
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
          const thumbPath = scene.videoPath ?? scene.imagePath;
          return (
            <button
              key={scene.id}
              type='button'
              className={`${styles.shotCard} ${active ? styles.shotCardActive : ''}`}
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
                />
                <span className='absolute bottom-6px left-6px rd-full bg-black/60 px-6px py-2px text-10px font-650 text-white'>
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
