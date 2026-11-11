export type AmbientRgb = readonly [number, number, number];

export interface AmbientPalette {
  dark: boolean;
  surface: AmbientRgb;
  accent: AmbientRgb;
  ink: AmbientRgb;
}

const LIGHT_PALETTE: AmbientPalette = {
  dark: false, surface: [245, 245, 247], accent: [66, 66, 69], ink: [29, 29, 31],
};
const DARK_PALETTE: AmbientPalette = {
  dark: true, surface: [44, 44, 46], accent: [209, 209, 214], ink: [245, 245, 247],
};
const THEME_STYLE_IDS = new Set(['user-defined-custom-css', 'theme-control-contract', 'user-wallpaper-tokens']);
const FRAME_LEAD_MS = 2;

export const ambientRgba = ([r, g, b]: AmbientRgb, alpha: number) =>
  `rgba(${r}, ${g}, ${b}, ${alpha})`;

export const mixAmbientColor = (a: AmbientRgb, b: AmbientRgb, weight: number): AmbientRgb => [
  Math.round(a[0] + (b[0] - a[0]) * weight),
  Math.round(a[1] + (b[1] - a[1]) * weight),
  Math.round(a[2] + (b[2] - a[2]) * weight),
];

export function ambientCanvasSize(width: number, height: number, dpr: number, maxPixels: number) {
  const scale = Math.min(dpr, 1.5, Math.sqrt(maxPixels / (width * height)));
  const pixelWidth = Math.min(maxPixels, Math.max(1, Math.floor(width * scale)));
  const pixelHeight = Math.max(1, Math.min(Math.floor(maxPixels / pixelWidth), Math.floor(height * scale)));
  return { pixelWidth, pixelHeight };
}

const isThemeStyle = (node: Node) =>
  node.nodeName === 'STYLE' && THEME_STYLE_IDS.has((node as HTMLStyleElement).id);

function readPalette(wall: HTMLElement, view: Window, doc: Document): AmbientPalette {
  const theme = doc.documentElement.getAttribute('data-theme')
    ?? doc.body.getAttribute('arco-theme')
    ?? doc.body.getAttribute('data-theme');
  const fallback = theme === 'dark' ? DARK_PALETTE : LIGHT_PALETTE;
  const pixel = doc.createElement('canvas');
  pixel.width = 1;
  pixel.height = 1;
  const sample = pixel.getContext('2d', { willReadFrequently: true });
  if (!sample) return fallback;

  const fallbackScope = doc.createElement('span');
  const probe = doc.createElement('span');
  fallbackScope.hidden = true;
  fallbackScope.appendChild(probe);
  wall.appendChild(fallbackScope);
  const read = (expression: string, defaultColor: AmbientRgb): AmbientRgb => {
    fallbackScope.style.color = ambientRgba(defaultColor, 1);
    probe.style.color = expression;
    const computed = view.getComputedStyle(probe).color;
    sample.clearRect(0, 0, 1, 1);
    // The browser converts modern CSS colors to sRGB once, outside the frame loop.
    sample.fillStyle = 'transparent';
    sample.fillStyle = computed;
    sample.fillRect(0, 0, 1, 1);
    const [r, g, b, alpha] = sample.getImageData(0, 0, 1, 1).data;
    return alpha ? [r, g, b] : defaultColor;
  };

  try {
    return {
      dark: fallback.dark,
      surface: read('var(--guid-canvas, var(--color-bg-1))', fallback.surface),
      accent: read('var(--wallpaper-accent, var(--color-primary, var(--brand)))', fallback.accent),
      ink: read('var(--color-text-1)', fallback.ink),
    };
  } finally {
    fallbackScope.remove();
  }
}

interface AmbientCanvasOptions {
  maxPixels: number;
  fps: number;
  resize: (rect: DOMRect) => void;
  setPalette: (palette: AmbientPalette) => void;
  draw: (time: number, delta: number) => void;
}

export function mountAmbientCanvas(
  wall: HTMLElement,
  canvas: HTMLCanvasElement,
  context: CanvasRenderingContext2D,
  options: AmbientCanvasOptions,
) {
  const doc = canvas.ownerDocument;
  const view = doc.defaultView!;
  const reduceMotion = view.matchMedia('(prefers-reduced-motion: reduce)');
  const forcedColors = view.matchMedia('(forced-colors: active)');
  const frameInterval = 1000 / options.fps;
  let frame: number | undefined;
  let timer: number | undefined;
  let bounds: DOMRect | undefined;
  let sizeDirty = true;
  let time = 0;
  let nextDrawAt = 0;
  let lastMotionFrame: number | undefined;
  let paused = false;
  let paletteDirty = true;
  let paletteKey = '';
  let needsDraw = true;
  let contextLost = false;
  let restorePending = false;

  const isMoving = () => !paused && !reduceMotion.matches;
  const hasWork = () => !doc.hidden && !forcedColors.matches && !contextLost
    && (sizeDirty || (bounds !== undefined && (isMoving() || paletteDirty || needsDraw)));

  const requestFrame = () => {
    timer = undefined;
    if (hasWork()) frame = view.requestAnimationFrame(tick);
  };
  const schedule = () => {
    if (frame !== undefined || timer !== undefined || !hasWork()) return;
    const delay = nextDrawAt - view.performance.now();
    if (delay > FRAME_LEAD_MS) {
      timer = view.setTimeout(requestFrame, Math.ceil(delay - FRAME_LEAD_MS));
    } else {
      requestFrame();
    }
  };

  const applySize = () => {
    sizeDirty = false;
    const rect = canvas.getBoundingClientRect();
    if (!rect.width || !rect.height) {
      bounds = undefined;
      lastMotionFrame = undefined;
      return;
    }
    const { pixelWidth, pixelHeight } = ambientCanvasSize(rect.width, rect.height, view.devicePixelRatio, options.maxPixels);
    const sameBounds = bounds?.width === rect.width && bounds.height === rect.height
      && bounds.left === rect.left && bounds.top === rect.top;
    bounds = rect;
    if (!restorePending && sameBounds && canvas.width === pixelWidth && canvas.height === pixelHeight) return;

    // Shrink height before growing width to keep intermediate backing sizes within budget.
    if (pixelHeight < canvas.height) canvas.height = pixelHeight;
    if (pixelWidth !== canvas.width) canvas.width = pixelWidth;
    if (pixelHeight !== canvas.height) canvas.height = pixelHeight;
    context.setTransform(pixelWidth / rect.width, 0, 0, pixelHeight / rect.height, 0, 0);
    options.resize(rect);
    restorePending = false;
    needsDraw = true;
  };

  const tick = (now: number) => {
    frame = undefined;
    if (!hasWork()) return;
    // The shared frame timestamp can precede the timer that requested this callback.
    if (view.performance.now() + FRAME_LEAD_MS < nextDrawAt) {
      schedule();
      return;
    }
    // Setting backing dimensions clears the bitmap. Commit the resize in the draw frame.
    if (sizeDirty) applySize();
    if (bounds === undefined) return;
    if (paletteDirty) {
      paletteDirty = false;
      const palette = readPalette(wall, view, doc);
      const key = JSON.stringify(palette);
      if (key !== paletteKey) {
        paletteKey = key;
        options.setPalette(palette);
        needsDraw = true;
      }
    }
    const moving = isMoving();
    if (moving || needsDraw) {
      const delta = moving && lastMotionFrame !== undefined ? Math.min(now - lastMotionFrame, 100) : 0;
      time += delta;
      lastMotionFrame = moving ? now : undefined;
      // Allow small refresh jitter without moving the next deadline forward.
      nextDrawAt = Math.max(nextDrawAt, now) + frameInterval;
      options.draw(time, delta);
      needsDraw = false;
    }
    schedule();
  };

  const resize = () => {
    sizeDirty = true;
    schedule();
  };

  const cancelScheduled = () => {
    if (frame !== undefined) view.cancelAnimationFrame(frame);
    if (timer !== undefined) view.clearTimeout(timer);
    frame = undefined;
    timer = undefined;
  };
  const syncMotion = () => {
    lastMotionFrame = undefined;
    cancelScheduled();
    schedule();
  };

  const loseContext = () => {
    contextLost = true;
    syncMotion();
  };
  const restoreContext = () => {
    contextLost = false;
    restorePending = true;
    paletteKey = '';
    paletteDirty = true;
    sizeDirty = true;
    syncMotion();
  };

  const themeObserver = new view.MutationObserver((records) => {
    if (!records.some((record) =>
      record.type === 'attributes' ||
      isThemeStyle(record.target) ||
      (record.target.parentNode !== null && isThemeStyle(record.target.parentNode)) ||
      [...record.addedNodes, ...record.removedNodes].some(isThemeStyle),
    )) return;
    paletteDirty = true;
    schedule();
  });
  themeObserver.observe(doc.documentElement, { attributes: true, attributeFilter: ['data-theme', 'data-color-scheme', 'data-wallpaper'] });
  themeObserver.observe(doc.body, { attributes: true, attributeFilter: ['arco-theme', 'data-theme'] });
  themeObserver.observe(doc.head, { subtree: true, childList: true, characterData: true });

  const resizeObserver = new view.ResizeObserver(resize);
  resizeObserver.observe(canvas);
  canvas.addEventListener('contextlost', loseContext);
  canvas.addEventListener('contextrestored', restoreContext);
  view.addEventListener('resize', resize);
  doc.addEventListener('visibilitychange', syncMotion);
  reduceMotion.addEventListener('change', syncMotion);
  forcedColors.addEventListener('change', syncMotion);
  schedule();

  return {
    setPaused(next: boolean) {
      if (paused === next) return;
      paused = next;
      syncMotion();
    },
    dispose() {
      cancelScheduled();
      resizeObserver.disconnect();
      themeObserver.disconnect();
      canvas.removeEventListener('contextlost', loseContext);
      canvas.removeEventListener('contextrestored', restoreContext);
      view.removeEventListener('resize', resize);
      doc.removeEventListener('visibilitychange', syncMotion);
      reduceMotion.removeEventListener('change', syncMotion);
      forcedColors.removeEventListener('change', syncMotion);
    },
  };
}
