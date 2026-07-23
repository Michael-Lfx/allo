

import React, { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Button, Spin } from '@arco-design/web-react';
import { Edit, Music, VideoOne } from '@icon-park/react';
import { getArtifact, loadArtifactMediaUrl } from '../api';
import {
  buildStoryboardScenes,
  findStoryboardPath,
  parseStoryboard,
  type StoryboardScene,
} from '../artifactPresentation';
import type { ArtifactNode } from '../types';
import styles from '../index.module.css';

interface StoryboardBoardProps {
  sessionId: string;
  artifacts: ArtifactNode[];
  disabled?: boolean;
  onReviseScene: (scene: StoryboardScene) => void;
}

interface SceneMediaProps {
  sessionId: string;
  path?: string;
  video?: boolean;
  compact?: boolean;
  alt: string;
}

const SceneMedia: React.FC<SceneMediaProps> = ({ sessionId, path, video, compact, alt }) => {
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

  if (!path || failed) {
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
  onReviseScene,
}) => {
  const { t } = useTranslation();
  const storyboardPath = useMemo(() => findStoryboardPath(artifacts), [artifacts]);
  const [storyboardText, setStoryboardText] = useState<string>();
  const [activeSceneId, setActiveSceneId] = useState<string>();

  useEffect(() => {
    if (!storyboardPath) {
      setStoryboardText(undefined);
      return;
    }
    let cancelled = false;
    void getArtifact(sessionId, storyboardPath)
      .then((content) => {
        if (!cancelled) setStoryboardText(content.text);
      })
      .catch(() => {
        if (!cancelled) setStoryboardText(undefined);
      });
    return () => {
      cancelled = true;
    };
  }, [sessionId, storyboardPath]);

  const scenes = useMemo(
    () =>
      buildStoryboardScenes(
        artifacts,
        parseStoryboard(storyboardText),
        storyboardPath
      ).slice(0, 30),
    [artifacts, storyboardPath, storyboardText]
  );
  const activeScene =
    scenes.find((scene) => scene.id === activeSceneId) ??
    scenes[0];

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
  const sceneNumber = activeScene.index + 1;

  return (
    <div className='flex flex-col gap-12px'>
      <div className={styles.storyStage}>
        <div className={styles.storyMedia}>
          <SceneMedia
            sessionId={sessionId}
            path={mainPath}
            video={Boolean(activeScene.videoPath)}
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
          <p className='m-0 text-14px leading-23px text-white/90'>
            {activeScene.visualDescription ||
              t('videoGeneration.studio.storyboard.visualPending', {
                defaultValue: '画面生成后将在这里展示。',
              })}
          </p>
          {activeScene.audioDescription ? (
            <div className='mt-14px flex items-start gap-7px border-t border-white/10 pt-12px text-12px leading-18px text-white/58'>
              <Music theme='outline' size={14} className='mt-2px shrink-0' />
              {activeScene.audioDescription}
            </div>
          ) : null}
          <Button
            className='!mt-18px !border-white/15 !bg-white/8 !text-white hover:!bg-white/14'
            disabled={disabled || !activeScene.revisionPath}
            onClick={() => onReviseScene(activeScene)}
          >
            <span className='inline-flex items-center gap-6px'>
              <Edit theme='outline' size={14} />
              {t('videoGeneration.studio.storyboard.reviseShot', {
                defaultValue: '修改这个镜头',
              })}
            </span>
          </Button>
        </aside>
      </div>

      <div className={styles.filmstrip} aria-label={t('videoGeneration.studio.storyboard.filmstrip', { defaultValue: '分镜胶片' })}>
        {scenes.map((scene) => {
          const number = scene.index + 1;
          const active = scene.id === activeScene.id;
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
                  path={scene.imagePath}
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
