import React, { useEffect, useRef } from 'react';
import { mountAmbientCanvas } from './ambientCanvas';
import { createAmbientScene, CREATE_AMBIENT_BUDGET, type CreateAmbientVariant } from './ambientCreateScene';
import styles from './CreateAmbientWall.module.css';

export type { CreateAmbientVariant } from './ambientCreateScene';

/** One bounded 2D scene. The shared scheduler owns every frame and pause gate. */
const CreateAmbientWall: React.FC<{ variant?: CreateAmbientVariant; paused?: boolean }> = ({
  variant = 'create',
  paused = false,
}) => {
  const wallRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const controlsRef = useRef<{ setPaused: (paused: boolean) => void } | null>(null);

  useEffect(() => {
    const wall = wallRef.current;
    const canvas = canvasRef.current;
    if (!wall || !canvas) return;
    const context = canvas.getContext('2d', { alpha: true, desynchronized: true });
    if (!context) return;
    const doc = canvas.ownerDocument;
    const view = doc.defaultView!;
    const motion = view.matchMedia('(prefers-reduced-motion: no-preference)');
    const fine = view.matchMedia('(hover: hover) and (pointer: fine)');
    const colors = view.matchMedia('(forced-colors: active)');
    const scene = createAmbientScene(context, variant);
    let bounds: DOMRect | undefined;

    const animation = mountAmbientCanvas(wall, canvas, context, {
      ...CREATE_AMBIENT_BUDGET,
      resize(rect) {
        bounds = rect;
        scene.resize(rect.width, rect.height);
      },
      setPalette: scene.setPalette,
      draw: scene.draw,
    });
    const reset = () => scene.resetPointer();
    const onMove = (event: PointerEvent) => {
      if (!bounds || doc.hidden || wall.dataset.paused === 'true'
        || !motion.matches || !fine.matches || colors.matches || event.pointerType === 'touch') return;
      const x = (event.clientX - bounds.left) / bounds.width;
      const y = (event.clientY - bounds.top) / bounds.height;
      if (x < 0 || x > 1 || y < 0 || y > 1) reset();
      else scene.setPointer(x, y);
    };
    controlsRef.current = {
      setPaused(next) {
        if (next) reset();
        animation.setPaused(next);
      },
    };
    view.addEventListener('pointermove', onMove, { passive: true });
    view.addEventListener('blur', reset);
    doc.documentElement.addEventListener('pointerleave', reset);
    doc.addEventListener('visibilitychange', reset);
    motion.addEventListener('change', reset);
    fine.addEventListener('change', reset);
    colors.addEventListener('change', reset);
    return () => {
      animation.dispose();
      controlsRef.current = null;
      view.removeEventListener('pointermove', onMove);
      view.removeEventListener('blur', reset);
      doc.documentElement.removeEventListener('pointerleave', reset);
      doc.removeEventListener('visibilitychange', reset);
      motion.removeEventListener('change', reset);
      fine.removeEventListener('change', reset);
      colors.removeEventListener('change', reset);
    };
  }, [variant]);

  useEffect(() => {
    controlsRef.current?.setPaused(paused);
  }, [paused, variant]);

  return (
    <div
      ref={wallRef}
      className={styles.wall}
      data-testid='guid-create-ambient'
      data-variant={variant}
      data-paused={paused ? 'true' : undefined}
      aria-hidden='true'
    >
      <canvas ref={canvasRef} className={styles.canvas} />
      <div className={styles.veil} />
    </div>
  );
};

export default CreateAmbientWall;
