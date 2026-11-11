import React, { useEffect, useRef } from 'react';
import { ambientRgba as rgba, mountAmbientCanvas, type AmbientRgb } from './ambientCanvas';
import styles from './BuildAmbientWall.module.css';

export type BuildAmbientVariant = 'directory' | 'website' | 'app' | 'game';

type Particle = {
  x: number;
  y: number;
  vx: number;
  vy: number;
  size: number;
  phase: number;
  z: number;
  previousX: number;
  previousY: number;
};

const MATRIX_GLYPHS = '01{}[]<>/\\=+-*;:constletfn=>ｱｲｳｴｵｶｷｸｹｺｻｼｽｾｿﾀﾁﾂﾃﾄﾅﾆﾇﾈﾉﾊﾋﾌﾍﾎﾏﾐﾑﾒﾓﾔﾕﾖﾗﾘﾙﾚﾛﾜﾝ';
const GLYPH_WIDTH = 16;
const GLYPH_HEIGHT = 20;
const ATLAS_COLUMNS = 16;
const ATLAS_ROWS = Math.ceil(MATRIX_GLYPHS.length / ATLAS_COLUMNS);

function createGlyphAtlas(doc: Document, accent: AmbientRgb, ink: AmbientRgb) {
  const atlas = doc.createElement('canvas');
  atlas.width = ATLAS_COLUMNS * GLYPH_WIDTH;
  atlas.height = ATLAS_ROWS * GLYPH_HEIGHT * 2;
  const context = atlas.getContext('2d');
  if (!context) return atlas;
  context.font = '12px ui-monospace, SFMono-Regular, Menlo, monospace';
  context.textAlign = 'center';
  context.textBaseline = 'top';
  for (const [layer, color] of [accent, ink].entries()) {
    context.fillStyle = rgba(color, 1);
    for (let index = 0; index < MATRIX_GLYPHS.length; index += 1) {
      const x = (index % ATLAS_COLUMNS) * GLYPH_WIDTH;
      const y = (Math.floor(index / ATLAS_COLUMNS) + layer * ATLAS_ROWS) * GLYPH_HEIGHT;
      context.fillText(MATRIX_GLYPHS[index], x + GLYPH_WIDTH / 2, y);
    }
  }
  return atlas;
}

/**
 * Full-bleed animated canvas for the build picker. It deliberately stays
 * outside the slide-card wall: one canvas, no per-card DOM and no external
 * visual dependency. The variant selects a different visual language, while
 * the pointer only nudges the existing field. Motion stops for reduced-motion
 * users and while the document is hidden.
 */
const BuildAmbientWall: React.FC<{ variant: BuildAmbientVariant }> = ({ variant }) => {
  const wallRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const wall = wallRef.current;
    const canvas = canvasRef.current;
    if (!wall || !canvas) return;
    const context = canvas.getContext('2d', { alpha: true, desynchronized: true });
    if (!context) return;

    let accent: AmbientRgb = [66, 66, 69];
    let ink: AmbientRgb = [29, 29, 31];
    const secondary: AmbientRgb = variant === 'game' ? [62, 226, 218] : [103, 187, 255];
    const pointer = { x: 0.5, y: 0.45, active: false };
    let width = 0;
    let height = 0;
    let wallRect: DOMRect;
    let particles: Particle[] = [];
    let waveStrokes: CanvasGradient[] = [];
    let glyphAtlas: HTMLCanvasElement;

    const seedParticles = () => {
      const count = variant === 'game' ? 48 : variant === 'app' ? 32 : 0;
      particles = [];
      for (let index = 0; index < count; index += 1) {
        particles.push({
          x: variant === 'game' ? (Math.random() - 0.5) * width * 0.42 : Math.random() * width,
          y: variant === 'game' ? (Math.random() - 0.5) * height * 0.42 : Math.random() * height,
          vx: (Math.random() - 0.5) * (variant === 'game' ? 0.8 : 0.28),
          vy: (Math.random() - 0.5) * (variant === 'game' ? 0.8 : 0.24),
          size: 0.6 + Math.random() * (variant === 'game' ? 1.8 : 1.4),
          phase: Math.random() * Math.PI * 2,
          z: 0.4 + Math.random() * 1.6,
          previousX: 0,
          previousY: 0,
        });
      }
    };

    const drawWebsite = (time: number) => {
      const drift = time * 0.00016;
      context.save();
      context.lineCap = 'round';
      for (let index = 0; index < 4; index += 1) {
        const baseY = height * (0.18 + index * 0.16);
        const amplitude = height * (0.035 + (index % 4) * 0.012);
        const phase = index * 0.9 + drift * (0.7 + index * 0.03);
        context.beginPath();
        for (let x = -36; x <= width + 36; x += 32) {
          const y = baseY
            + Math.sin(x * 0.004 + phase) * amplitude
            + Math.sin(x * 0.0013 - phase * 1.7) * amplitude * 0.55;
          if (x === -36) context.moveTo(x, y);
          else context.lineTo(x, y);
        }
        let stroke = waveStrokes[index];
        if (!stroke) {
          stroke = context.createLinearGradient(0, 0, width, 0);
          stroke.addColorStop(0, rgba(accent, 0));
          stroke.addColorStop(0.28, rgba(accent, 0.3 + (index % 3) * 0.05));
          stroke.addColorStop(0.62, rgba(accent, 0.38 - (index % 2) * 0.04));
          stroke.addColorStop(1, rgba(ink, 0));
          waveStrokes[index] = stroke;
        }
        context.strokeStyle = stroke;
        context.lineWidth = index % 3 === 0 ? 2.4 : 1;
        context.stroke();
      }
      context.globalAlpha = 1;
      context.restore();
    };

    const drawApp = (time: number, delta: number) => {
      const step = delta * 0.03;
      const pointerX = pointer.x * width;
      const pointerY = pointer.y * height;
      const pulse = 0.5 + Math.sin(time * 0.0018) * 0.5;
      for (const particle of particles) {
        particle.x += particle.vx * step;
        particle.y += particle.vy * step;
        if (pointer.active) {
          const dx = pointerX - particle.x;
          const dy = pointerY - particle.y;
          const distance = Math.max(1, Math.hypot(dx, dy));
          particle.x += (dx / distance) * 0.18 * step;
          particle.y += (dy / distance) * 0.18 * step;
        }
        if (particle.x < -20) particle.x = width + 20;
        if (particle.x > width + 20) particle.x = -20;
        if (particle.y < -20) particle.y = height + 20;
        if (particle.y > height + 20) particle.y = -20;
      }
      context.save();
      for (let index = 0; index < particles.length; index += 1) {
        const from = particles[index];
        let links = 0;
        for (let otherIndex = index + 1; otherIndex < particles.length && links < 2; otherIndex += 1) {
          const to = particles[otherIndex];
          const dx = from.x - to.x;
          const dy = from.y - to.y;
          const distanceSquared = dx * dx + dy * dy;
          if (distanceSquared > 126 * 126) continue;
          const distance = Math.sqrt(distanceSquared);
          links += 1;
          context.globalAlpha = (1 - distance / 126) * 0.42;
          context.strokeStyle = rgba(accent, 0.95);
          context.lineWidth = 0.7;
          context.beginPath();
          context.moveTo(from.x, from.y);
          context.lineTo(to.x, to.y);
          context.stroke();
        }
      }
      for (const particle of particles) {
        const distanceToPointer = Math.hypot(particle.x - pointerX, particle.y - pointerY);
        const highlight = pointer.active && distanceToPointer < 220 ? 0.68 + pulse * 0.22 : 0.42;
        context.globalAlpha = 1;
        context.fillStyle = rgba(accent, highlight);
        context.beginPath();
        context.arc(particle.x, particle.y, particle.size + (distanceToPointer < 160 ? 1.2 : 0), 0, Math.PI * 2);
        context.fill();
      }
      if (pointer.active) {
        context.globalAlpha = 0.16;
        context.strokeStyle = rgba(accent, 0.9);
        context.lineWidth = 1;
        context.beginPath();
        context.arc(pointerX, pointerY, 72 + pulse * 18, 0, Math.PI * 2);
        context.stroke();
      }
      context.restore();
    };

    const drawGame = (delta: number) => {
      const horizon = height * 0.64;
      const vanishX = width * (0.5 + (pointer.x - 0.5) * 0.035);
      context.save();
      context.globalAlpha = 0.72;
      context.strokeStyle = rgba(accent, 1);
      context.lineWidth = 1.3;
      for (let index = 0; index < 15; index += 1) {
        const targetX = width * (index / 14);
        context.beginPath();
        context.moveTo(vanishX, horizon);
        context.lineTo(targetX, height + 24);
        context.stroke();
      }
      for (let index = 1; index < 9; index += 1) {
        const y = horizon + Math.pow(index / 9, 2.2) * (height - horizon) * 1.04;
        context.beginPath();
        context.moveTo(0, y);
        context.lineTo(width, y);
        context.stroke();
      }
      context.globalAlpha = 0.9;
      context.strokeStyle = rgba(secondary, 0.8);
      context.lineWidth = 1.6;
      context.beginPath();
      context.moveTo(0, horizon);
      context.lineTo(width, horizon);
      context.stroke();

      for (const particle of particles) {
        particle.z -= 0.0045 * delta * 0.03;
        if (particle.z < 0.18) {
          particle.z = 1.8;
          particle.x = (Math.random() - 0.5) * width * 1.5;
          particle.y = (Math.random() - 0.5) * height * 1.5;
        }
        const scale = 1 / particle.z;
        const projectedX = vanishX + particle.x * scale;
        const projectedY = horizon + particle.y * scale;
        if (particle.previousX && particle.previousY) {
          context.globalAlpha = Math.min(0.95, (1.8 - particle.z) * 0.58);
          context.strokeStyle = rgba(particle.phase > Math.PI ? secondary : accent, 1);
          context.lineWidth = Math.max(0.8, particle.size * 1.35);
          context.beginPath();
          context.moveTo(particle.previousX, particle.previousY);
          context.lineTo(projectedX, projectedY);
          context.stroke();
        }
        particle.previousX = projectedX;
        particle.previousY = projectedY;
        if (projectedX > 0 && projectedX < width && projectedY > 0 && projectedY < height) {
          context.globalAlpha = 0.65;
          context.fillStyle = rgba(particle.phase > Math.PI ? secondary : accent, 0.9);
          context.beginPath();
          context.arc(projectedX, projectedY, Math.max(0.7, particle.size * 0.7), 0, Math.PI * 2);
          context.fill();
        }
      }

      context.restore();
    };

    const drawDirectory = (time: number) => {
      const seconds = time * 0.001;
      const columnWidth = Math.max(38, width / 24);
      const columns = Math.ceil(width / columnWidth);
      context.save();
      for (let column = 0; column < columns; column += 1) {
        const seed = ((column * 9301 + 49297) % 233280) / 233280;
        const speed = 34 + seed * 62;
        const head = ((seconds * speed + seed * height * 1.8) % (height + 240)) - 120;
        const glyphFrame = Math.floor(seconds * (4 + seed * 5));
        for (let row = 0; row < 6; row += 1) {
          const y = head - row * GLYPH_HEIGHT;
          if (y < -GLYPH_HEIGHT || y > height) continue;
          const glyphIndex = (column * 7 + row + glyphFrame) % MATRIX_GLYPHS.length;
          const sourceX = (glyphIndex % ATLAS_COLUMNS) * GLYPH_WIDTH;
          const layer = row === 0 ? 0 : ATLAS_ROWS;
          const sourceY = (Math.floor(glyphIndex / ATLAS_COLUMNS) + layer) * GLYPH_HEIGHT;
          context.globalAlpha = row === 0 ? 0.52 : (1 - row / 6) * 0.22;
          context.drawImage(
            glyphAtlas, sourceX, sourceY, GLYPH_WIDTH, GLYPH_HEIGHT,
            column * columnWidth + (columnWidth - GLYPH_WIDTH) / 2, y, GLYPH_WIDTH, GLYPH_HEIGHT,
          );
        }
      }
      context.restore();
    };

    const pointerMotion = window.matchMedia('(prefers-reduced-motion: no-preference)');
    const onPointerMove = (event: PointerEvent) => {
      if (!wallRect || !pointerMotion.matches || document.hidden) return;
      pointer.x = Math.min(1, Math.max(0, (event.clientX - wallRect.left) / width));
      pointer.y = Math.min(1, Math.max(0, (event.clientY - wallRect.top) / height));
      pointer.active = true;
    };

    const animation = mountAmbientCanvas(wall, canvas, context, {
      maxPixels: 500_000,
      fps: 20,
      resize(rect) {
        const resized = width !== rect.width || height !== rect.height;
        width = rect.width;
        height = rect.height;
        wallRect = rect;
        waveStrokes = [];
        if (resized) seedParticles();
      },
      setPalette(palette) {
        accent = palette.accent;
        ink = palette.ink;
        waveStrokes = [];
        if (variant === 'directory') glyphAtlas = createGlyphAtlas(canvas.ownerDocument, accent, ink);
      },
      draw(time, delta) {
        context.clearRect(0, 0, width, height);
        if (variant === 'website') drawWebsite(time);
        else if (variant === 'app') drawApp(time, delta);
        else if (variant === 'game') drawGame(delta);
        else drawDirectory(time);
      },
    });

    if (variant === 'app' || variant === 'game') {
      window.addEventListener('pointermove', onPointerMove, { passive: true });
    }
    return () => {
      animation.dispose();
      window.removeEventListener('pointermove', onPointerMove);
    };
  }, [variant]);

  return (
    <div ref={wallRef} className={styles.wall} data-variant={variant} aria-hidden='true'>
      <canvas ref={canvasRef} className={styles.canvas} />
      <div className={styles.veil} />
    </div>
  );
};

export default BuildAmbientWall;
