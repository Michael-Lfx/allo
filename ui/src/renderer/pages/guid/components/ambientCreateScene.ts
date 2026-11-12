import { ambientRgba, mixAmbientColor, type AmbientPalette } from './ambientCanvas';

export type CreateAmbientVariant = 'create' | 'image' | 'video';

export const CREATE_AMBIENT_BUDGET = { maxPixels: 500_000, fps: 20 } as const;
const TAU = Math.PI * 2;
const FOLD_STEPS = 64;
const EXPOSURE_COLUMNS = 18;
const EXPOSURE_ROWS = 10;
const FILM_FRAMES = 14;

type FoldPoint = { x: number; y: number };
const foldPoint = (): FoldPoint => ({ x: 0, y: 0 });

// Stable identities across resize, theme change and remount. No random frame work.
const seed = (index: number, salt: number) => {
  let value = Math.imul(index + salt * 101, 374761393);
  value = Math.imul(value ^ (value >>> 13), 1274126177);
  return ((value ^ (value >>> 16)) >>> 0) / 4294967296;
};

/** Scene geometry only: no DOM reads/writes, timers, textures or pixel readbacks. */
export function createAmbientScene(context: CanvasRenderingContext2D, variant: CreateAmbientVariant) {
  let width = 1;
  let height = 1;
  let primary = '';
  let secondary = '';
  let ink = '';
  let material = '';
  let paper: CanvasGradient;
  let pigments: string[] = [];
  let foldTones: string[][] = [];
  let dark = false;
  const pointer = { x: 0.5, y: 0.35, targetX: 0.5, targetY: 0.35, active: false };
  const folds = variant === 'create' ? Array.from({ length: FOLD_STEPS + 1 }, (_, index) => ({
    u: index / FOLD_STEPS,
    center: foldPoint(),
    edges: Array.from({ length: 3 }, foldPoint),
    tangents: Array.from({ length: 3 }, foldPoint),
    twist: 0,
    light: 0,
  })) : [];
  const shavings = variant === 'create' ? Array.from({ length: 18 }, (_, index) => ({
    u: seed(index, 11),
    offset: (index % 2 ? 1 : -1) * (0.65 + seed(index, 12) * 0.55),
    size: 0.6 + seed(index, 13) * 0.6,
    phase: seed(index, 14) * TAU,
    near: index >= 12,
    center: foldPoint(),
    tangent: foldPoint(),
  })) : [];
  const chips = variant === 'image' ? Array.from({ length: 36 }, (_, index) => ({
    u: seed(index, 1), v: seed(index, 2), size: 3 + seed(index, 3) * 9,
  })) : [];
  const exposure = variant === 'image' ? Array.from({ length: EXPOSURE_ROWS }, (_, row) => {
    const cells = [];
    for (let column = 0; column < EXPOSURE_COLUMNS;) {
      const span = column < 6 ? 2 : 1;
      const u = column / EXPOSURE_COLUMNS;
      const v = row / EXPOSURE_ROWS;
      const identity = row * EXPOSURE_COLUMNS + column;
      if (seed(identity, 7) < 0.38 + u * 0.7) {
        const ridge = 0.4 + Math.sin(u * 9) * 0.13 + Math.cos(u * 17) * 0.06;
        cells.push({ u, v, span, tone: v < ridge ? (v < 0.2 ? 0 : 1) : (v < ridge + 0.2 ? 2 : (u + v > 1.3 ? 4 : 3)), grain: seed(identity, 8) });
      }
      column += span;
    }
    return cells;
  }).flat() : [];

  const proximity = (x: number, y: number) => {
    if (!pointer.active) return 0;
    const radius = Math.min(280, Math.max(140, width * 0.18));
    const dx = x - pointer.x * width;
    const dy = y - pointer.y * height;
    return Math.max(0, 1 - (dx * dx + dy * dy) / (radius * radius));
  };
  const line = (x1: number, y1: number, x2: number, y2: number) => {
    context.moveTo(x1, y1);
    context.lineTo(x2, y2);
  };

  // A periodic C-infinity wave packet: no crest reset or cusp at the loop boundary.
  function foldCenter(u: number, time: number, point: FoldPoint) {
    const envelope = Math.sin(u * Math.PI) ** 2;
    const phase = u * TAU - time * TAU / 18_000 - 1.2;
    const lift = ((1 + Math.cos(phase)) * 0.5) ** 6;
    point.x = width * (0.03 + u * 0.94);
    point.y = height * (0.23 + Math.sin(u * TAU * 1.15 - 1.1) * 0.075
      + Math.sin(u * TAU * 2 - time * 0.00016) * 0.016
      - lift * envelope * 0.035);
    return lift;
  }

  function foldInfluence(x: number, y: number) {
    if (!pointer.active || pointer.y >= 0.58) return 0;
    const radius = Math.min(width * 0.2, height * 0.25, 240);
    const distance = ((x - pointer.x * width) ** 2 + (y - pointer.y * height) ** 2) / (radius * radius);
    const support = Math.max(0, 1 - distance);
    const upper = Math.min(1, (0.58 - pointer.y) / 0.16);
    // Both the spatial support and the input-region gate have zero first/second derivatives.
    return support ** 3 * upper ** 3 * (10 - 15 * upper + 6 * upper * upper);
  }

  function drawShavings(time: number, near: boolean) {
    const span = Math.min(width * 0.07, height * 0.06, 72);
    for (const shaving of shavings) {
      if (shaving.near !== near) continue;
      const u = (shaving.u + time * 0.000013 * (near ? 1.08 : 0.76)) % 1;
      const point = shaving.center;
      foldCenter(u, time, point);
      foldCenter(u + 0.001, time, shaving.tangent);
      const slope = Math.atan2(shaving.tangent.y - point.y, shaving.tangent.x - point.x);
      const offset = span * shaving.offset * (near ? 1.35 : 1.65);
      const x = point.x - Math.sin(slope) * offset;
      const y = point.y + Math.cos(slope) * offset;
      const w = Math.min(width * 0.008, height * 0.012, 12) * shaving.size * (near ? 1.25 : 0.7);
      const h = w * (0.22 + Math.sin(time * 0.00024 + shaving.phase) * 0.08);
      const alpha = Math.sin(u * Math.PI) ** 2 * (near ? 0.62 : 0.28);
      context.save();
      context.translate(x, y);
      context.rotate(slope + Math.sin(time * 0.00018 + shaving.phase) * 0.22);
      context.globalAlpha = alpha;
      context.fillStyle = foldTones[0][76];
      context.beginPath();
      context.moveTo(-w, 0);
      context.lineTo(w, 0);
      context.lineTo(-w * 0.35, h);
      context.closePath();
      context.fill();
      context.fillStyle = foldTones[1][38];
      context.beginPath();
      context.moveTo(-w, 0);
      context.lineTo(-w * 0.25, -h);
      context.lineTo(w, 0);
      context.closePath();
      context.fill();
      context.strokeStyle = primary;
      context.lineWidth = near ? 0.75 : 0.5;
      context.globalAlpha = alpha * 0.5;
      context.beginPath();
      line(-w, 0, w, 0);
      context.stroke();
      context.restore();
    }
  }

  // Every face and outline uses these same cubic edges, in either direction.
  function traceFold(edge: number, reverse = false) {
    for (let step = 1; step <= FOLD_STEPS; step += 1) {
      const from = folds[reverse ? FOLD_STEPS - step + 1 : step - 1];
      const to = folds[reverse ? FOLD_STEPS - step : step];
      const sign = reverse ? -1 : 1;
      const a = from.edges[edge];
      const b = to.edges[edge];
      const da = from.tangents[edge];
      const db = to.tangents[edge];
      context.bezierCurveTo(a.x + sign * da.x / 3, a.y + sign * da.y / 3,
        b.x - sign * db.x / 3, b.y - sign * db.y / 3, b.x, b.y);
    }
  }

  function fillFold(first: number, second: number) {
    context.beginPath();
    context.moveTo(folds[0].edges[first].x, folds[0].edges[first].y);
    traceFold(first);
    context.lineTo(folds[FOLD_STEPS].edges[second].x, folds[FOLD_STEPS].edges[second].y);
    traceFold(second, true);
    context.closePath();
    context.fill();
  }

  function drawFoldFlow(time: number) {
    const unit = Math.min(width, height * 1.1);
    const span = Math.min(width * 0.07, height * 0.06, 72);
    for (const fold of folds) {
      const { u, center, edges } = fold;
      const lift = foldCenter(u, time, center);
      fold.light = foldInfluence(center.x, center.y);
      center.y -= fold.light * unit * 0.021;
      fold.twist = u * TAU * 0.82 - 0.6 + lift * 0.5
        + Math.sin(u * TAU - time * 0.0001) * 0.16
        + fold.light * (pointer.x - u) * 0.22;
      const facing = Math.cos(fold.twist);
      const halfWidth = span * Math.sin(u * Math.PI) ** 2
        * (0.92 + Math.sin(u * TAU - time * 0.00012) * 0.08);
      for (let edge = 0; edge < 3; edge += 1) {
        // Monotonic shared x prevents different lengths of the sheet from intersecting.
        // The ridge also collapses at an edge-on turn; it cannot protrude through a face.
        edges[edge].x = center.x;
        edges[edge].y = center.y + halfWidth * facing * (edge - 1
          - (edge === 1 ? Math.sin(fold.twist) * 0.16 : 0));
      }
    }
    for (let index = 0; index <= FOLD_STEPS; index += 1) {
      const previous = folds[Math.max(0, index - 1)];
      const next = folds[Math.min(FOLD_STEPS, index + 1)];
      const divisor = index === 0 || index === FOLD_STEPS ? 1 : 2;
      for (let edge = 0; edge < 3; edge += 1) {
        folds[index].tangents[edge].x = (next.edges[edge].x - previous.edges[edge].x) / divisor;
        folds[index].tangents[edge].y = (next.edges[edge].y - previous.edges[edge].y) / divisor;
      }
    }
    drawShavings(time, false);
    context.globalAlpha = 1;
    // A single opaque silhouette under both complete faces, never overlapping panel patches.
    context.fillStyle = foldTones[0][64];
    fillFold(0, 2);
    for (let face = 0; face < 2; face += 1) {
      const shade = context.createLinearGradient(width * 0.03, 0, width * 0.97, 0);
      for (const fold of folds) {
        const normal = 0.5 + Math.cos(fold.twist + (face === 0 ? -0.32 : 0.32)) * 0.5;
        const tone = Math.round(Math.min(1, Math.max(0, normal + fold.light * 0.06)) * 128);
        shade.addColorStop(fold.u, foldTones[face][tone]);
      }
      context.fillStyle = shade;
      fillFold(face, face + 1);
    }
    for (let edge = 0; edge < 3; edge += 1) {
      context.strokeStyle = edge === 1 ? secondary : primary;
      context.lineWidth = edge === 1 ? 0.85 : 0.65;
      context.globalAlpha = dark ? (edge === 1 ? 0.42 : 0.32) : (edge === 1 ? 0.32 : 0.24);
      context.beginPath();
      context.moveTo(folds[0].edges[edge].x, folds[0].edges[edge].y);
      traceFold(edge);
      context.stroke();
    }
    drawShavings(time, true);
  }

  function drawExposure(time: number) {
    const w = Math.min(width * 0.61, height * 0.79, 800);
    const h = w * 0.49;
    const cx = width * 0.6;
    const cy = height * 0.245;
    for (let layer = 2; layer >= 0; layer -= 1) {
      context.save();
      context.translate(cx - layer * w * 0.16, cy - layer * h * 0.12);
      context.transform(1, -0.13 + layer * 0.09, 0.18, 1, 0, 0);
      context.lineWidth = 1.2;
      context.strokeStyle = layer === 1 ? secondary : primary;
      context.fillStyle = paper;
      context.globalAlpha = layer === 0 ? 0.62 : 0.24;
      context.beginPath();
      context.rect(-w / 2, -h / 2, w, h);
      context.fill();
      context.globalAlpha = layer === 0 ? 0.36 : 0.22;
      context.stroke();
      if (layer !== 0) {
        context.beginPath();
        for (let row = 0; row < 5; row += 1) {
          line(-w * 0.44, h * (-0.35 + row * 0.16), -w * 0.27, h * (-0.35 + row * 0.16));
        }
        context.stroke();
      } else {
        for (const cell of exposure) {
          const x = (cell.u - 0.5) * w;
          const y = (cell.v - 0.5) * h;
          const light = proximity(cx + x + y * 0.18, cy + y - x * 0.13);
          const develop = 0.5 + 0.5 * Math.sin(time * 0.00032 - cell.u * 4 + cell.v);
          context.globalAlpha = Math.min(0.85, 0.21 + cell.u * 0.25 + develop * 0.12 + light * 0.28);
          context.fillStyle = pigments[cell.tone];
          context.beginPath();
          const gap = 0.5 + (1 - cell.u) * (1 - cell.u) * 3;
          context.rect(x + gap / 2, y + gap / 2, Math.max(0.1, w * cell.span / EXPOSURE_COLUMNS - gap), Math.max(0.1, h / EXPOSURE_ROWS - gap));
          context.fill();
          if (light > 0.25) {
            context.globalAlpha = light * 0.6;
            context.strokeStyle = ink;
            context.beginPath();
            line(x + 5, y + h * 0.065, x + w / EXPOSURE_COLUMNS * 0.8, y + h * (0.03 + cell.grain * 0.025));
            context.stroke();
          }
        }
        context.strokeStyle = secondary;
        context.globalAlpha = dark ? 0.55 : 0.44;
        for (let ridge = 0; ridge < 3; ridge += 1) {
          context.beginPath();
          for (let step = 0; step <= 40; step += 1) {
            const u = step / 40;
            const x = (u - 0.5) * w;
            const y = h * (-0.1 + ridge * 0.17 + Math.sin(u * 9 + ridge * 0.5) * (0.13 - ridge * 0.035) + Math.cos(u * 17) * 0.035);
            if (step === 0) context.moveTo(x, y);
            else context.lineTo(x, y);
          }
          context.stroke();
        }
      }
      context.strokeStyle = ink;
      context.globalAlpha = layer === 0 ? 0.52 : 0.22;
      context.beginPath();
      for (const side of [-1, 1]) {
        for (const edge of [-1, 1]) {
          const x = side * (w / 2 + 7);
          const y = edge * (h / 2 + 7);
          line(x - side * 14, y, x, y);
          line(x, y, x, y - edge * 14);
        }
      }
      context.stroke();
      context.restore();
    }
    for (let index = 0; index < chips.length; index += 1) {
      const chip = chips[index];
      const progress = (chip.u + time * 0.000025) % 1;
      const x = cx - w * (0.98 - progress * 0.48);
      const y = cy + (chip.v - 0.5) * h * 1.05 - progress * h * 0.12;
      const size = chip.size * Math.min(1, width / 880);
      context.save();
      context.translate(x, y);
      context.rotate(-0.13 + chip.v * 0.4);
      context.globalAlpha = Math.sin(progress * Math.PI) * 0.46;
      context.fillStyle = pigments[index % pigments.length];
      context.beginPath();
      context.rect(-size, -size * 0.4, size * 2, size * 0.8);
      context.fill();
      context.restore();
    }
  }

  // Perspective film rails: nearby frames are wider, all three bands stay parallel.
  function filmPoint(u: number, lane: number, edge: number) {
    const x = width * (u * 1.4 - 0.2);
    const center = height * (0.24 + lane * 0.24 - u * 0.42 + Math.sin(u * 3.2 + lane * 0.3) * 0.12);
    const halfWidth = Math.min(100, height * 0.085) * (0.45 + u * 0.7);
    const light = proximity(x, center);
    return {
      x: x + (pointer.x - 0.5) * (lane + 1) * 9,
      y: center + edge * halfWidth + (pointer.y - 0.35) * 18 + light * (pointer.y * height - center) * 0.12,
    };
  }
  function drawFilm(time: number) {
    const advance = (time * 0.000012) % (1 / FILM_FRAMES);
    const firstFrame = Math.floor(time * 0.000012 * FILM_FRAMES);
    for (let lane = 0; lane < 3; lane += 1) {
      context.strokeStyle = lane === 1 ? secondary : primary;
      context.lineWidth = 1.5;
      context.globalAlpha = dark ? 0.38 : 0.28;
      context.beginPath();
      for (const edge of [-1, -0.76, 0.76, 1]) {
        for (let step = 0; step <= 40; step += 1) {
          const point = filmPoint(step / 40, lane, edge);
          if (step === 0) context.moveTo(point.x, point.y);
          else context.lineTo(point.x, point.y);
        }
      }
      context.stroke();
      for (let frame = 0; frame < FILM_FRAMES; frame += 1) {
        const u = frame / FILM_FRAMES + advance;
        const a = filmPoint(u, lane, -0.76);
        const b = filmPoint(u, lane, 0.76);
        const light = proximity(a.x, (a.y + b.y) / 2);
        context.globalAlpha = 0.22 + light * 0.45;
        context.beginPath();
        line(a.x, a.y, b.x, b.y);
        // Abstract storyboard landscapes; no images, text or offscreen textures.
        for (let step = 0; step <= 8; step += 1) {
          const t = step / 8;
          const point = filmPoint(u + (0.012 + t * 0.048), lane,
            0.15 + Math.sin(t * Math.PI * 2 + frame - firstFrame + lane) * 0.28);
          if (step === 0) context.moveTo(point.x, point.y);
          else context.lineTo(point.x, point.y);
        }
        context.stroke();
      }
      context.globalAlpha = 0.38;
      context.fillStyle = secondary;
      context.beginPath();
      for (let hole = 0; hole < FILM_FRAMES * 3; hole += 1) {
        const u = hole / (FILM_FRAMES * 3) + advance;
        for (const edge of [-0.88, 0.88]) {
          const point = filmPoint(u, lane, edge);
          context.rect(point.x - 1.5, point.y - 1.5, 3, 3);
        }
      }
      context.fill();
    }
  }

  return {
    resize(nextWidth: number, nextHeight: number) {
      width = nextWidth;
      height = nextHeight;
    },
    setPalette(palette: AmbientPalette) {
      dark = palette.dark;
      const copper = palette.dark ? [224, 166, 116] as const : [147, 88, 51] as const;
      const cyan = palette.dark ? [108, 195, 202] as const : [39, 111, 128] as const;
      primary = ambientRgba(mixAmbientColor(palette.accent,
        variant === 'image' ? copper : palette.ink, variant === 'image' ? 0.78 : 0.3), 1);
      secondary = ambientRgba(mixAmbientColor(palette.accent, variant === 'create' ? copper : cyan, 0.68), 1);
      ink = ambientRgba(palette.ink, 1);
      if (variant !== 'video') {
        material = ambientRgba(mixAmbientColor(palette.surface, palette.accent, dark ? 0.08 : 0.04), 1);
        paper = context.createLinearGradient(-250, -120, 250, 120);
        paper.addColorStop(0, ambientRgba(mixAmbientColor(palette.surface, palette.accent, dark ? 0.18 : 0.08), 1));
        paper.addColorStop(1, ambientRgba(mixAmbientColor(palette.surface, palette.ink, dark ? 0.045 : 0.025), 1));
        pigments = [copper, mixAmbientColor(copper, palette.accent, 0.45), palette.accent,
          mixAmbientColor(palette.accent, cyan, 0.65), cyan].map((tone) => ambientRgba(tone, 1));
      }
      if (variant === 'create') {
        foldTones = [0, 1].map((face) => {
          const front = mixAmbientColor(palette.surface, palette.accent, dark ? 0.7 - face * 0.07 : 0.48 - face * 0.05);
          const back = mixAmbientColor(palette.surface,
            mixAmbientColor(palette.accent, copper, 0.8), dark ? 0.4 + face * 0.07 : 0.22 + face * 0.04);
          return Array.from({ length: 129 }, (_, tone) => ambientRgba(mixAmbientColor(back, front, tone / 128), 1));
        });
      }
    },
    setPointer(x: number, y: number) {
      pointer.targetX = Math.max(0, Math.min(1, x));
      pointer.targetY = Math.max(0, Math.min(1, y));
      pointer.active = true;
    },
    resetPointer() {
      pointer.x = pointer.targetX = 0.5;
      pointer.y = pointer.targetY = 0.35;
      pointer.active = false;
    },
    draw(time: number, delta: number) {
      const ease = 1 - Math.exp(-delta / 130);
      pointer.x += (pointer.targetX - pointer.x) * ease;
      pointer.y += (pointer.targetY - pointer.y) * ease;
      context.clearRect(0, 0, width, height);
      context.save();
      context.lineCap = 'round';
      context.lineJoin = 'round';
      if (variant === 'create') drawFoldFlow(time);
      else if (variant === 'image') drawExposure(time);
      else drawFilm(time);
      context.restore();
    },
  };
}
