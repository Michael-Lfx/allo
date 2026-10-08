import React, { useEffect, useRef } from 'react';
import { useWallpaper } from '@renderer/hooks/ui/useWallpaper';

/**
 * One full-bleed atmosphere node for the main window. Canvas stays an opaque
 * island via wallpaperScene.css; the companion overlay never mounts this.
 */
const WallpaperScene: React.FC = () => {
  const { prefs, scene, analysis } = useWallpaper();
  const videoRef = useRef<HTMLVideoElement>(null);
  const reduceMotion =
    typeof window !== 'undefined' && window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  const freezeMotion = reduceMotion || prefs.reducedMotionPolicy === 'freeze' || !prefs.videoEnabled;

  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    const onVisibility = () => {
      if (document.hidden || freezeMotion) {
        video.pause();
      } else {
        void video.play().catch(() => undefined);
      }
    };
    onVisibility();
    document.addEventListener('visibilitychange', onVisibility);
    return () => document.removeEventListener('visibilitychange', onVisibility);
  }, [freezeMotion, scene.url]);

  if (!prefs.enabled || scene.kind === 'none' || !analysis) {
    return null;
  }

  return (
    <>
      <div id='wallpaper-scene' aria-hidden='true'>
        {scene.kind === 'image' && scene.url ? (
          <img
            src={freezeMotion && scene.posterUrl ? scene.posterUrl : scene.url}
            alt=''
            draggable={false}
          />
        ) : null}
        {scene.kind === 'video' && scene.url ? (
          <video
            ref={videoRef}
            src={scene.url}
            poster={scene.posterUrl}
            muted
            loop
            playsInline
            autoPlay={!freezeMotion}
          />
        ) : null}
      </div>
      <div id='wallpaper-scrim' aria-hidden='true' />
    </>
  );
};

export default WallpaperScene;
