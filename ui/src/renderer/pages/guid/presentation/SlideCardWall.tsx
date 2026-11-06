/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { WIDE_PATTERNS } from './slideWallPatterns';
import styles from './SlideCardWall.module.css';

/** Grid pitch and card box, in px (cards are centered in their cell). */
const PITCH_X = 92;
const PITCH_Y = 64;
const CARD_W = 68;
const CARD_H = 38;
/** Click wave: runs over the whole wall, this many ms per cell of distance. */
const WAVE_STEP_MS = 55;
/** Within this many cells (about three rings) cards do a full flip and change picture. */
const WAVE_FLIP_RINGS = 3.2;
/** Beyond that, cards only squeeze a little (fraction of width), fading with distance. */
const WAVE_SQUEEZE_MAX = 0.12;
const WAVE_SQUEEZE_MIN = 0.03;
/** Brightening that rides on the wavefront, mostly over the flipping rings (cells). */
const WAVE_GLOW_REACH = 4.5;
/** Ambient shimmer: a soft patch of cards fades in and out every few seconds. */
const AMBIENT_EVERY_MS = 2200;
const AMBIENT_RADIUS = 1.8;
/** Pointer position meaning "no hover": far off the grid, so every glow is 0. */
const OFF_GRID = -100;

type Cell = {
  key: string;
  col: number;
  row: number;
  /** Pattern index into WIDE_PATTERNS. A blank card keeps it hidden until lit. */
  pattern: number;
  /** Empty frame at rest; its wireframe only shows while light is on it. */
  blank: boolean;
  /** Resting ink strength, 0–1, so the wall is unevenly faded like a real board. */
  ink: number;
  /** Entrance delay (ms), radiating from the bottom center. */
  enter: number;
};

/** Stable per-cell noise so a resize keeps every card's picture. */
function noise(col: number, row: number, salt: number): number {
  let h = (col * 374761393 + row * 668265263 + salt * 2147483647) | 0;
  h = Math.imul(h ^ (h >>> 13), 1274126177);
  return ((h ^ (h >>> 16)) >>> 0) / 4294967296;
}

function buildCells(cols: number, rows: number): Cell[] {
  const cells: Cell[] = [];
  const centerCol = (cols - 1) / 2;
  for (let row = 0; row < rows; row += 1) {
    for (let col = 0; col < cols; col += 1) {
      // Offsets from the center keep a cell's identity when the wall grows.
      const c = Math.round(col - centerCol);
      const blank = noise(c, row, 2) < 0.32;
      cells.push({
        key: `${c}:${row}`,
        col,
        row,
        pattern: Math.floor(noise(c, row, 3) * WIDE_PATTERNS.length),
        blank,
        ink: noise(c, row, 4),
        enter: Math.min(900, Math.round(Math.hypot(col - centerCol, rows - 1 - row) * 38)),
      });
    }
  }
  return cells;
}

const PATTERN_VARS: React.CSSProperties = Object.fromEntries(WIDE_PATTERNS.map((value, index) => [`--wp${index}`, value]));

const patternVar = (index: number) => `var(--wp${index})`;

/** Give `card` a different random wireframe (written to the DOM; React never re-renders for it). */
function shufflePattern(card: HTMLElement) {
  const count = WIDE_PATTERNS.length;
  let next = Math.floor(Math.random() * count);
  if (next === Number(card.dataset.pattern)) next = (next + 1) % count;
  card.dataset.pattern = String(next);
  card.style.setProperty('--pat', patternVar(next));
}

/** Click: a new wireframe that stays visible, even on a card that was blank. */
function switchPattern(card: HTMLElement) {
  shufflePattern(card);
  delete card.dataset.blank;
}

/**
 * Flip-styled switch: squeeze the face to a sliver, swap the picture while it
 * is invisible, open it again. Deliberately 2D (scale, no rotateY/perspective):
 * hundreds of simultaneous 3D layers made Chromium/WebView2 flash black tiles
 * when waves overlapped. Cards already mid-flip are left alone.
 */
function flipCard(card: HTMLElement, delay: number, swap: (card: HTMLElement) => void): void {
  const face = card.firstElementChild as HTMLElement | null;
  if (!face || card.dataset.flipping !== undefined) return;
  card.dataset.flipping = '';
  face
    .animate([{ transform: 'scale(1, 1)' }, { transform: 'scale(0, 0.94)' }], {
      duration: 150,
      delay,
      easing: 'cubic-bezier(0.4, 0, 1, 1)',
      fill: 'forwards',
    })
    .finished.then((half) => {
      swap(card);
      const back = face.animate([{ transform: 'scale(0, 0.94)' }, { transform: 'scale(1, 1)' }], {
        duration: 320,
        easing: 'cubic-bezier(0.2, 1.25, 0.4, 1)',
      });
      half.cancel();
      return back.finished;
    })
    .catch(() => undefined)
    .finally(() => {
      delete card.dataset.flipping;
    });
}

/** Far part of the wave: a slight squeeze and back (a hint of a flip), no picture change. */
function squeezeCard(card: HTMLElement, delay: number, amount: number): void {
  const face = card.firstElementChild as HTMLElement | null;
  if (!face || card.dataset.flipping !== undefined) return;
  face.animate(
    [{ transform: 'scaleX(1)' }, { transform: `scaleX(${(1 - amount).toFixed(3)})` }, { transform: 'scaleX(1)' }],
    { duration: 460, delay, easing: 'ease-in-out' }
  );
}

/** Wave flip for the other cards: pictures change, blank frames stay blank. */
function waveSwap(card: HTMLElement): void {
  if (card.dataset.blank === undefined) shufflePattern(card);
}

function prefersReducedMotion(): boolean {
  return typeof window !== 'undefined' && window.matchMedia('(prefers-reduced-motion: reduce)').matches;
}

/**
 * Decorative wall of slide wireframes behind the "Create presentation" panel.
 * Hover lifts the card under the pointer and reveals its neighbors with a soft
 * falloff (blank frames show a hidden wireframe only while lit); a click
 * switches the card to another wireframe with a flip, and a wave spreads over
 * the whole wall: full flips in the nearest three rings, a fading squeeze beyond; now and then a small patch of cards
 * shimmers in and out on its own.
 *
 * Performance: one React render per resize. Hover writes two CSS variables on
 * the wall (rAF-throttled) and every card derives its own glow in CSS. The
 * wave and the shimmer are Web Animations: flips animate transforms only, and
 * the glow rides on registered custom properties of just the cards near the
 * click or the shimmer. Patterns are shared mask images, so each card is two
 * divs and no SVG DOM.
 */
const SlideCardWall: React.FC = () => {
  const wallRef = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ cols: 0, rows: 0, offsetX: 0, offsetY: 0 });

  useEffect(() => {
    const wall = wallRef.current;
    if (!wall) return;
    const measure = () => {
      const { width, height } = wall.getBoundingClientRect();
      // One extra column on each side so the grid bleeds off the edges.
      const cols = Math.ceil(width / PITCH_X) + 2;
      const rows = Math.ceil(height / PITCH_Y) + 1;
      const offsetX = Math.round((width - cols * PITCH_X) / 2);
      const offsetY = Math.round(Math.min(0, (height - rows * PITCH_Y) / 2));
      setSize((current) =>
        current.cols === cols && current.rows === rows && current.offsetX === offsetX && current.offsetY === offsetY
          ? current
          : { cols, rows, offsetX, offsetY }
      );
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(wall);
    return () => observer.disconnect();
  }, []);

  const cells = useMemo(() => buildCells(size.cols, size.rows), [size.cols, size.rows]);

  // --- Hover: pointer position in cell units, written straight to CSS. ---
  useEffect(() => {
    const wall = wallRef.current;
    if (!wall) return;
    const hoverable = window.matchMedia('(hover: hover) and (pointer: fine)');
    let frame = 0;
    let clientX = 0;
    let clientY = 0;
    const write = (x: number, y: number) => {
      wall.style.setProperty('--px', x.toFixed(3));
      wall.style.setProperty('--py', y.toFixed(3));
    };
    const onMove = (event: PointerEvent) => {
      if (!hoverable.matches) return;
      clientX = event.clientX;
      clientY = event.clientY;
      if (frame) return;
      frame = window.requestAnimationFrame(() => {
        frame = 0;
        const rect = wall.getBoundingClientRect();
        write((clientX - rect.left - size.offsetX) / PITCH_X - 0.5, (clientY - rect.top - size.offsetY) / PITCH_Y - 0.5);
      });
    };
    const onLeave = () => {
      if (frame) window.cancelAnimationFrame(frame);
      frame = 0;
      write(OFF_GRID, OFF_GRID);
    };
    // The wall sits under the composer; listen on the page so the light follows
    // the pointer through the transparent gaps around the panel.
    const host = wall.parentElement ?? wall;
    host.addEventListener('pointermove', onMove, { passive: true });
    host.addEventListener('pointerleave', onLeave);
    return () => {
      host.removeEventListener('pointermove', onMove);
      host.removeEventListener('pointerleave', onLeave);
      if (frame) window.cancelAnimationFrame(frame);
    };
  }, [size.offsetX, size.offsetY]);

  // --- Click: the card flips to a new wireframe, then a flip wave runs over the whole wall. ---
  const handleClick = useCallback((event: React.MouseEvent<HTMLDivElement>) => {
    const wall = wallRef.current;
    const card = (event.target as HTMLElement).closest<HTMLElement>('[data-cell]');
    if (!wall || !card || card.dataset.flipping !== undefined) return;
    if (prefersReducedMotion()) {
      switchPattern(card);
      return;
    }
    flipCard(card, 0, switchPattern);

    const col = Number(card.dataset.col);
    const row = Number(card.dataset.row);
    for (const other of wall.querySelectorAll<HTMLElement>('[data-cell]')) {
      if (other === card) continue;
      const distance = Math.hypot(Number(other.dataset.col) - col, (Number(other.dataset.row) - row) * 1.25);
      const delay = distance * WAVE_STEP_MS;
      if (distance <= WAVE_FLIP_RINGS) {
        flipCard(other, delay, waveSwap);
      } else {
        // Squeeze fades from WAVE_SQUEEZE_MAX just outside the rings down to WAVE_SQUEEZE_MIN.
        const fade = Math.exp(-(distance - WAVE_FLIP_RINGS) / 3);
        squeezeCard(other, delay, WAVE_SQUEEZE_MIN + (WAVE_SQUEEZE_MAX - WAVE_SQUEEZE_MIN) * fade);
      }
      // A short brightening rides on the wavefront near the click.
      const glow = 1 - distance / WAVE_GLOW_REACH;
      if (glow > 0) {
        other.animate([{ '--wall-ripple': 0 }, { '--wall-ripple': glow * 0.8 }, { '--wall-ripple': 0 }] as Keyframe[], {
          duration: 520,
          delay,
          easing: 'ease-out',
        });
      }
    }
  }, []);

  // --- Ambient shimmer: now and then a few cards light up and fade again. ---
  useEffect(() => {
    const wall = wallRef.current;
    if (!wall || size.cols === 0 || prefersReducedMotion()) return;
    const spark = () => {
      if (document.hidden) return;
      // Keep the patch out of the bottom center, where the panel covers the wall.
      let col = 0;
      let row = 0;
      for (let attempt = 0; attempt < 6; attempt += 1) {
        col = Math.random() * size.cols;
        row = Math.random() * size.rows;
        const underPanel = Math.abs(col - size.cols / 2) < size.cols * 0.24 && row > size.rows * 0.4;
        if (!underPanel) break;
      }
      for (const card of wall.querySelectorAll<HTMLElement>('[data-cell]')) {
        if (card.dataset.ambient !== undefined) continue;
        const distance = Math.hypot(Number(card.dataset.col) - col, (Number(card.dataset.row) - row) * 1.25) / AMBIENT_RADIUS;
        if (distance >= 1) continue;
        // A blank card reveals a fresh random wireframe each time it shimmers.
        if (card.dataset.blank !== undefined) shufflePattern(card);
        const peak = Math.pow(1 - distance, 1.4) * 0.85;
        card.dataset.ambient = '';
        card
          .animate([{ '--wall-ambient': 0 }, { '--wall-ambient': peak }, { '--wall-ambient': 0 }] as Keyframe[], {
            duration: 4200,
            delay: distance * 240,
            easing: 'ease-in-out',
          })
          .finished.catch(() => undefined)
          .finally(() => {
            delete card.dataset.ambient;
          });
      }
    };
    const first = window.setTimeout(spark, 1100);
    const timer = window.setInterval(spark, AMBIENT_EVERY_MS);
    return () => {
      window.clearTimeout(first);
      window.clearInterval(timer);
    };
  }, [size.cols, size.rows]);

  const wallStyle = useMemo<React.CSSProperties>(
    () => ({ ...PATTERN_VARS, '--px': OFF_GRID, '--py': OFF_GRID }) as React.CSSProperties,
    []
  );

  return (
    <div ref={wallRef} className={styles.wall} style={wallStyle} aria-hidden='true' onClick={handleClick} data-testid='guid-slide-card-wall'>
      {cells.map((cell) => {
        const style = {
          '--c': cell.col,
          '--r': cell.row,
          '--ink': cell.ink.toFixed(2),
          '--enter': `${cell.enter}ms`,
          left: size.offsetX + cell.col * PITCH_X + (PITCH_X - CARD_W) / 2,
          top: size.offsetY + cell.row * PITCH_Y + (PITCH_Y - CARD_H) / 2,
          width: CARD_W,
          '--pat': patternVar(cell.pattern),
        } as React.CSSProperties;
        return (
          <div
            key={cell.key}
            className={styles.card}
            style={style}
            data-cell=''
            data-col={cell.col}
            data-row={cell.row}
            data-pattern={cell.pattern}
            data-blank={cell.blank ? '' : undefined}
          >
            <div className={styles.face} />
          </div>
        );
      })}
    </div>
  );
};

export default SlideCardWall;
