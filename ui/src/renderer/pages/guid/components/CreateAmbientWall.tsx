import React, { useEffect, useRef } from 'react';
import { ambientRgba, mixAmbientColor, mountAmbientCanvas, type AmbientRgb } from './ambientCanvas';
import styles from './CreateAmbientWall.module.css';

type CreateAmbientVariant = 'create' | 'image' | 'video';

interface Light {
  tint: AmbientRgb;
  x: number;
  y: number;
  width: number;
  height: number;
  driftX: number;
  driftY: number;
  phase: number;
}

const LIGHTS: Record<CreateAmbientVariant, readonly Light[]> = {
  create: [
    { tint: [167, 116, 247], x: 0.36, y: 0.34, width: 0.9, height: 1.05, driftX: 0.16, driftY: 0.12, phase: 0 },
    { tint: [105, 192, 246], x: 0.73, y: 0.42, width: 0.72, height: 0.86, driftX: 0.13, driftY: 0.1, phase: 2.8 },
  ],
  image: [
    { tint: [250, 158, 98], x: 0.22, y: 0.5, width: 0.7, height: 1.15, driftX: 0.12, driftY: 0.13, phase: 0.7 },
    { tint: [228, 114, 159], x: 0.79, y: 0.4, width: 0.66, height: 1.12, driftX: 0.1, driftY: 0.16, phase: 2.6 },
  ],
  video: [
    { tint: [109, 183, 235], x: 0.51, y: 0.42, width: 1.02, height: 0.54, driftX: 0.24, driftY: 0.035, phase: 0.4 },
    { tint: [230, 132, 101], x: 0.25, y: 0.5, width: 0.58, height: 0.39, driftX: 0.2, driftY: 0.025, phase: 2.3 },
  ],
};

function createGlow(doc: Document, color: AmbientRgb, strength: number) {
  const sprite = doc.createElement('canvas');
  sprite.width = sprite.height = 128;
  const context = sprite.getContext('2d');
  if (!context) return sprite;
  const glow = context.createRadialGradient(64, 64, 0, 64, 64, 64);
  glow.addColorStop(0, ambientRgba(color, strength));
  glow.addColorStop(0.45, ambientRgba(color, strength * 0.48));
  glow.addColorStop(1, ambientRgba(color, 0));
  context.fillStyle = glow;
  context.fillRect(0, 0, 128, 128);
  return sprite;
}

const CreateAmbientWall: React.FC<{ variant?: CreateAmbientVariant; paused?: boolean }> = ({
  variant = 'create',
  paused = false,
}) => {
  const wallRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const animationRef = useRef<ReturnType<typeof mountAmbientCanvas> | null>(null);

  useEffect(() => {
    const wall = wallRef.current;
    const canvas = canvasRef.current;
    const context = canvas?.getContext('2d');
    if (!wall || !canvas || !context) return;

    const lights = LIGHTS[variant];
    let sprites: HTMLCanvasElement[] = [];
    let width = 0;
    let height = 0;
    const animation = mountAmbientCanvas(wall, canvas, context, {
      maxPixels: 300_000,
      fps: 15,
      resize(rect) {
        width = rect.width;
        height = rect.height;
      },
      setPalette(palette) {
        const strength = palette.dark ? 0.42 : 0.3;
        sprites = lights.map((light, index) => {
          const tint = mixAmbientColor(light.tint, palette.accent, 0.24);
          return createGlow(canvas.ownerDocument, tint, strength * (index === 0 ? 1 : 0.72));
        });
      },
      draw(time) {
        context.clearRect(0, 0, width, height);
        for (const [index, sprite] of sprites.entries()) {
          const light = lights[index];
          const phase = time * 0.00042 + light.phase;
          const w = width * light.width;
          const h = height * light.height;
          const x = width * (light.x + Math.sin(phase) * light.driftX);
          const y = height * (light.y + Math.cos(phase * 0.8) * light.driftY);
          context.drawImage(sprite, x - w / 2, y - h / 2, w, h);
        }
      },
    });
    animationRef.current = animation;
    return () => {
      animation.dispose();
      animationRef.current = null;
    };
  }, [variant]);

  useEffect(() => {
    animationRef.current?.setPaused(paused);
  }, [paused, variant]);

  return (
    <div ref={wallRef} className={styles.wall} data-testid='guid-create-ambient' data-variant={variant} aria-hidden='true'>
      <canvas ref={canvasRef} className={styles.canvas} />
    </div>
  );
};

export default CreateAmbientWall;
