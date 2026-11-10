import React, { useEffect, useRef } from 'react';
import styles from './BuildAmbientWall.module.css';

export type BuildAmbientVariant = 'directory' | 'website' | 'app' | 'game';

type Rgb = [number, number, number];
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

const FALLBACK_ACCENT: Rgb = [124, 108, 255];
const FALLBACK_INK: Rgb = [180, 174, 210];
const CODE_GLYPHS = '01<>/{}[]();:=+-*constletfn=>';
const MATRIX_GLYPHS = '01{}[]<>/\\=+-*;:constletfn=>ｱｲｳｴｵｶｷｸｹｺｻｼｽｾｿﾀﾁﾂﾃﾄﾅﾆﾇﾈﾉﾊﾋﾌﾍﾎﾏﾐﾑﾒﾓﾔﾕﾖﾗﾘﾙﾚﾛﾜﾝ';

const rgba = (color: Rgb, alpha: number) =>
  `rgba(${Math.round(color[0])}, ${Math.round(color[1])}, ${Math.round(color[2])}, ${alpha})`;

const readToken = (element: HTMLElement, token: string, fallback: Rgb): Rgb => {
  const probe = document.createElement('span');
  probe.style.color = `var(${token})`;
  probe.style.display = 'none';
  element.appendChild(probe);
  const value = getComputedStyle(probe).color;
  probe.remove();
  const match = value.match(/rgba?\(([^)]+)\)/);
  if (!match) return fallback;
  const parts = match[1].split(/[ ,/]+/).filter(Boolean).map(Number);
  return parts.length >= 3 ? [parts[0], parts[1], parts[2]] : fallback;
};

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
  const [themeRevision, setThemeRevision] = React.useState(0);

  // Re-read CSS tokens and rebuild the field when the host theme changes.
  useEffect(() => {
    const observer = new MutationObserver(() => setThemeRevision((value) => value + 1));
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
    observer.observe(document.body, { attributes: true, attributeFilter: ['arco-theme', 'data-theme'] });
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    const wall = wallRef.current;
    const canvas = canvasRef.current;
    if (!wall || !canvas) return;
    const context = canvas.getContext('2d', { alpha: true, desynchronized: true });
    if (!context) return;

    const accent = readToken(wall, '--brand', FALLBACK_ACCENT);
    const ink = readToken(wall, '--color-text-1', FALLBACK_INK);
    const isDark =
      document.documentElement.getAttribute('data-theme') === 'dark' ||
      document.body.getAttribute('arco-theme') === 'dark';
    const secondary: Rgb = variant === 'game' ? [62, 226, 218] : [103, 187, 255];
    const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    const pointer = { x: 0.5, y: 0.45, active: false };
    let width = 0;
    let height = 0;
    let wallRect = { left: 0, top: 0, width: 0, height: 0 };
    let dpr = 1;
    let frame = 0;
    let particles: Particle[] = [];
    let lastTime = 0;

    const seedParticles = () => {
      const count = variant === 'game' ? 110 : variant === 'app' ? 68 : variant === 'website' ? 38 : 0;
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

    const resize = () => {
      const rect = wall.getBoundingClientRect();
      width = Math.max(1, Math.round(rect.width));
      height = Math.max(1, Math.round(rect.height));
      wallRect = { left: rect.left, top: rect.top, width: rect.width, height: rect.height };
      dpr = Math.min(window.devicePixelRatio || 1, 1.5);
      canvas.width = Math.round(width * dpr);
      canvas.height = Math.round(height * dpr);
      context.setTransform(dpr, 0, 0, dpr, 0, 0);
      seedParticles();
      render(reducedMotion ? 0 : performance.now(), true);
    };

    const drawWebsite = (time: number) => {
      const drift = time * 0.00016;
      context.save();
      context.lineCap = 'round';
      for (let index = 0; index < 9; index += 1) {
        const baseY = height * (0.14 + index * 0.085);
        const amplitude = height * (0.035 + (index % 4) * 0.012);
        const phase = index * 0.9 + drift * (0.7 + index * 0.03);
        context.beginPath();
        for (let x = -36; x <= width + 36; x += 18) {
          const y = baseY
            + Math.sin(x * 0.004 + phase) * amplitude
            + Math.sin(x * 0.0013 - phase * 1.7) * amplitude * 0.55;
          if (x === -36) context.moveTo(x, y);
          else context.lineTo(x, y);
        }
        const stroke = context.createLinearGradient(0, 0, width, 0);
        stroke.addColorStop(0, rgba(accent, 0));
        stroke.addColorStop(0.28, rgba(accent, 0.3 + (index % 3) * 0.05));
        stroke.addColorStop(0.62, rgba(accent, 0.38 - (index % 2) * 0.04));
        stroke.addColorStop(1, rgba(ink, 0));
        context.strokeStyle = stroke;
        context.lineWidth = index % 3 === 0 ? 2.4 : 1;
        context.stroke();
      }
      context.globalAlpha = 1;
      context.restore();
    };

    const drawApp = (time: number) => {
      const pointerX = pointer.x * width;
      const pointerY = pointer.y * height;
      const pulse = 0.5 + Math.sin(time * 0.0018) * 0.5;
      for (const particle of particles) {
        particle.x += particle.vx;
        particle.y += particle.vy;
        if (pointer.active) {
          const dx = pointerX - particle.x;
          const dy = pointerY - particle.y;
          const distance = Math.max(1, Math.hypot(dx, dy));
          particle.x += (dx / distance) * 0.18;
          particle.y += (dy / distance) * 0.18;
        }
        if (particle.x < -20) particle.x = width + 20;
        if (particle.x > width + 20) particle.x = -20;
        if (particle.y < -20) particle.y = height + 20;
        if (particle.y > height + 20) particle.y = -20;
      }
      context.save();
      for (let index = 0; index < particles.length; index += 1) {
        const from = particles[index];
        for (let otherIndex = index + 1; otherIndex < particles.length; otherIndex += 1) {
          const to = particles[otherIndex];
          const distance = Math.hypot(from.x - to.x, from.y - to.y);
          if (distance > 126) continue;
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
      context.globalAlpha = 0.12;
      context.strokeStyle = rgba(ink, 0.7);
      for (let y = height * 0.1; y < height; y += 96) {
        context.beginPath();
        context.moveTo(0, y);
        context.lineTo(width, y);
        context.stroke();
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

    const drawGame = (time: number) => {
      const horizon = height * 0.64;
      const vanishX = width * (0.5 + (pointer.x - 0.5) * 0.035);
      context.save();
      const stage = context.createLinearGradient(0, 0, 0, height);
      stage.addColorStop(0, isDark ? 'rgba(4, 3, 12, 1)' : 'rgba(8, 5, 20, 1)');
      stage.addColorStop(0.55, isDark ? 'rgba(10, 5, 24, 1)' : 'rgba(18, 8, 36, 1)');
      stage.addColorStop(0.68, isDark ? 'rgba(16, 7, 30, 1)' : 'rgba(30, 12, 48, 1)');
      stage.addColorStop(1, isDark ? 'rgba(2, 2, 6, 1)' : 'rgba(5, 3, 12, 1)');
      context.fillStyle = stage;
      context.fillRect(0, 0, width, height);
      const sky = context.createLinearGradient(0, 0, 0, height);
      sky.addColorStop(0, rgba(accent, isDark ? 0.035 : 0.055));
      sky.addColorStop(0.56, rgba(secondary, isDark ? 0.025 : 0.04));
      sky.addColorStop(0.64, rgba(accent, isDark ? 0.2 : 0.25));
      sky.addColorStop(0.67, rgba(secondary, isDark ? 0.05 : 0.075));
      sky.addColorStop(1, rgba(ink, isDark ? 0.02 : 0.035));
      context.fillStyle = sky;
      context.fillRect(0, 0, width, height);

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
        particle.z -= 0.0045;
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

      const glow = context.createRadialGradient(vanishX, horizon, 0, vanishX, horizon, width * 0.54);
      glow.addColorStop(0, rgba(accent, 0.42));
      glow.addColorStop(0.45, rgba(accent, 0.1));
      glow.addColorStop(1, rgba(accent, 0));
      context.globalAlpha = 1;
      context.fillStyle = glow;
      context.fillRect(0, 0, width, height);
      context.restore();
    };

    const drawDirectory = (time: number) => {
      const seconds = time * 0.001;
      const columnWidth = 38;
      const lineHeight = 20;
      const columns = Math.ceil(width / columnWidth);
      context.save();
      context.font = '12px ui-monospace, SFMono-Regular, Menlo, monospace';
      context.textAlign = 'center';
      context.textBaseline = 'top';
      const headStyle = rgba(accent, 0.52);
      const trailStyles = new Map<number, string[]>();
      const styleForRow = (row: number, trail: number) => {
        if (row === 0) return headStyle;
        let styles = trailStyles.get(trail);
        if (!styles) {
          styles = Array.from({ length: trail }, (_, index) =>
            rgba(ink, Math.max(0, 1 - index / trail) * 0.22),
          );
          trailStyles.set(trail, styles);
        }
        return styles[row] ?? rgba(ink, 0.02);
      };

      // Transparent Matrix-style code rain: no canvas background fill, only
      // falling glyphs so the picker remains the visual anchor.
      for (let column = 0; column < columns; column += 1) {
        const seed = ((column * 9301 + 49297) % 233280) / 233280;
        const speed = 34 + seed * 62;
        const head = ((seconds * speed + seed * height * 1.8) % (height + 240)) - 120;
        const trail = 8 + (column % 4);
        const glyphFrame = Math.floor(seconds * (4 + seed * 5));
        for (let row = 0; row < trail; row += 1) {
          const y = head - row * lineHeight;
          if (y < -20 || y > height + 20) continue;
          const glyphIndex = (column * 7 + row + glyphFrame) % MATRIX_GLYPHS.length;
          context.fillStyle = styleForRow(row, trail);
          context.fillText(
            MATRIX_GLYPHS[glyphIndex],
            column * columnWidth + columnWidth / 2,
            y,
          );
        }
      }
      context.restore();
    };
    const render = (time: number, force = false) => {
      if (!width || !height) return;
      if (!force && document.hidden) return;
      context.clearRect(0, 0, width, height);
      const transparentVariant = variant === 'directory';
      if (!transparentVariant) {
        const wash = context.createLinearGradient(0, 0, width, height);
        wash.addColorStop(0, rgba(accent, isDark ? 0.13 : 0.075));
        wash.addColorStop(0.5, rgba(ink, isDark ? 0.025 : 0.014));
        wash.addColorStop(1, rgba(variant === 'game' ? secondary : accent, isDark ? 0.055 : 0.032));
        context.fillStyle = wash;
        context.fillRect(0, 0, width, height);
      }
      if (variant === 'website') drawWebsite(time);
      else if (variant === 'app') drawApp(time);
      else if (variant === 'game') drawGame(time);
      else drawDirectory(time);
      if (!transparentVariant) {
        const glow = context.createRadialGradient(
          pointer.x * width,
          pointer.y * height,
          0,
          pointer.x * width,
          pointer.y * height,
          Math.max(width, height) * 0.45,
        );
        glow.addColorStop(0, rgba(accent, pointer.active ? 0.16 : 0.08));
        glow.addColorStop(0.45, rgba(accent, pointer.active ? 0.05 : 0.025));
        glow.addColorStop(1, rgba(accent, 0));
        context.fillStyle = glow;
        context.fillRect(0, 0, width, height);
      }
    };

    const onPointerMove = (event: PointerEvent) => {
      const rect = wallRect;
      pointer.x = Math.min(1, Math.max(0, (event.clientX - rect.left) / Math.max(1, rect.width)));
      pointer.y = Math.min(1, Math.max(0, (event.clientY - rect.top) / Math.max(1, rect.height)));
      pointer.active = true;
      if (reducedMotion) render(0, true);
    };

    const onVisibilityChange = () => {
      if (!document.hidden && !reducedMotion) {
        if (frame) window.cancelAnimationFrame(frame);
        lastTime = performance.now();
        frame = window.requestAnimationFrame(tick);
      }
    };

    const tick = (time: number) => {
      if (document.hidden) {
        frame = 0;
        return;
      }
      if (time - lastTime > 28) {
        render(time);
        lastTime = time;
      }
      frame = window.requestAnimationFrame(tick);
    };

    const resizeObserver = new ResizeObserver(resize);
    resizeObserver.observe(wall);
    window.addEventListener('pointermove', onPointerMove, { passive: true });
    document.addEventListener('visibilitychange', onVisibilityChange);
    resize();
    if (!reducedMotion) frame = window.requestAnimationFrame(tick);

    return () => {
      resizeObserver.disconnect();
      window.removeEventListener('pointermove', onPointerMove);
      document.removeEventListener('visibilitychange', onVisibilityChange);
      if (frame) window.cancelAnimationFrame(frame);
    };
  }, [themeRevision, variant]);

  return (
    <div ref={wallRef} className={styles.wall} data-variant={variant} aria-hidden='true'>
      <canvas ref={canvasRef} className={styles.canvas} />
      <div className={styles.veil} />
    </div>
  );
};

export default BuildAmbientWall;
