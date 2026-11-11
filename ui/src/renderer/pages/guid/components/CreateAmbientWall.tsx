import React, { useEffect, useRef } from 'react';
import { ambientRgba, mixAmbientColor, mountAmbientCanvas, type AmbientRgb } from './ambientCanvas';
import styles from './CreateAmbientWall.module.css';

type CreateAmbientVariant = 'create' | 'image' | 'video';

function traceWaves(context: CanvasRenderingContext2D, width: number, height: number, time: number) {
  for (let row = 0; row < 3; row += 1) {
    const center = height * (0.26 + row * 0.23);
    const phase = time / 1100 + row * 0.9;
    for (let segment = 0; segment <= 24; segment += 1) {
      const x = width * segment / 24;
      const y = center + Math.sin(segment * 0.37 + phase) * height * 0.075;
      if (segment === 0) context.moveTo(x, y);
      else context.lineTo(x, y);
    }
  }
}

function traceDots(context: CanvasRenderingContext2D, width: number, height: number, time: number) {
  for (let row = 0; row < 3; row += 1) {
    for (let column = 0; column < 16; column += 1) {
      const pulse = Math.sin(time / 950 + column * 0.56 + row * 0.9);
      const x = width * (column + 0.5) / 16;
      const y = height * (0.28 + row * 0.21 + pulse * 0.055);
      const radius = 1.1 + pulse * 0.65;
      context.moveTo(x - radius, y);
      context.lineTo(x + radius, y);
    }
  }
}

function traceTimeline(context: CanvasRenderingContext2D, width: number, height: number, time: number) {
  const spacing = width / 14;
  for (let row = 0; row < 3; row += 1) {
    const y = height * (0.3 + row * 0.2);
    const offset = (time * (0.014 + row * 0.003) + row * spacing / 3) % (spacing * 2);
    for (let segment = -2; segment < 14; segment += 1) {
      const start = segment * spacing + offset;
      const length = spacing * (segment % 2 === 0 ? 0.62 : 0.26);
      context.moveTo(start, y);
      context.lineTo(start + length, y);
    }
  }
}

const PATTERNS: Record<CreateAmbientVariant, { tint: AmbientRgb; trace: typeof traceWaves }> = {
  create: { tint: [144, 118, 220], trace: traceWaves },
  image: { tint: [210, 129, 107], trace: traceDots },
  video: { tint: [104, 161, 203], trace: traceTimeline },
};

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

    const { tint, trace } = PATTERNS[variant];
    let ink = 'transparent';
    let width = 0;
    let height = 0;
    const animation = mountAmbientCanvas(wall, canvas, context, {
      maxPixels: 48_000,
      fps: 10,
      resize(rect) {
        width = rect.width;
        height = rect.height;
      },
      setPalette(palette) {
        ink = ambientRgba(mixAmbientColor(tint, palette.accent, 0.24), palette.dark ? 0.48 : 0.4);
      },
      draw(time) {
        context.clearRect(0, 0, width, height);
        context.strokeStyle = ink;
        context.lineWidth = 1.5;
        context.lineCap = 'round';
        context.beginPath();
        trace(context, width, height, time);
        context.stroke();
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
