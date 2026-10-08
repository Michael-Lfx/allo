import { describe, expect, test } from 'bun:test';
import { DEFAULT_WALLPAPER_PREFS, type WallpaperAnalysis } from '@/common/types/appearance';
import { buildWallpaperTokenCss, plateContrastRatio, resolveWallpaperDim } from './wallpaperTokens';
import { normalizeWallpaperPrefs, resolvedWallpaperId } from './wallpaperPrefs';

const analysis: WallpaperAnalysis = {
  version: 1,
  seedHex: '#112233',
  meanLstar: 40,
  luminanceVariance: 0.3,
  recommendedScheme: 'dark',
  recommendedDim: 0.4,
  recommendedBlurPx: 8,
  recommendedPlateAlpha: 0.8,
  primaryRgb: [20, 80, 180],
  primaryScale: [
    [200, 210, 240],
    [160, 180, 220],
    [120, 150, 200],
    [80, 120, 180],
    [40, 90, 160],
    [20, 80, 180],
    [10, 40, 90],
  ],
  busy: true,
  animated: false,
};

describe('wallpaper tokens', () => {
  test('disabled wallpaper emits no token sheet', () => {
    expect(
      buildWallpaperTokenCss({
        enabled: false,
        prefs: DEFAULT_WALLPAPER_PREFS,
        analysis,
        scheme: 'dark',
      })
    ).toBe('');
  });

  test('writes opaque-safe variables and never alpha text tokens', () => {
    const css = buildWallpaperTokenCss({
      enabled: true,
      prefs: { ...DEFAULT_WALLPAPER_PREFS, enabled: true, harmonizeAccent: true },
      analysis,
      scheme: 'dark',
    });
    expect(css.includes("--wallpaper-seed: #112233")).toBe(true);
    expect(css.includes('--text-primary')).toBe(false);
    expect(css.includes('--color-text-1')).toBe(false);
    expect(css.includes('--primary-6:')).toBe(true);
    expect(css.includes('--wallpaper-chrome-alpha: 0')).toBe(true);
    expect(css.includes('.layout-content')).toBe(true);
    expect(css.includes('.layout-sider')).toBe(true);
    expect(css.includes('.workspace-tool-rail')).toBe(true);
    expect(css.includes('.preview-panel-shell')).toBe(true);
    expect(css.includes('.file-change-group')).toBe(true);
    expect(css.includes('.d2h-file-header')).toBe(true);
    expect(css.includes('.xterm-viewport')).toBe(true);
    expect(css.includes('--terminal-surface-bg: transparent')).toBe(true);
    expect(css.includes('.oc-portal-host')).toBe(true);
    expect(css.includes('[data-infinite-canvas]')).toBe(true);
    expect(css.includes('--oc-canvas-fill')).toBe(false);
    expect(css.includes('background: transparent !important')).toBe(true);
  });

  test('auto dim uses ingest recommendation', () => {
    expect(resolveWallpaperDim({ ...DEFAULT_WALLPAPER_PREFS, dim: 'auto' }, analysis)).toBe(0.4);
    expect(resolveWallpaperDim({ ...DEFAULT_WALLPAPER_PREFS, dim: 0.1 }, analysis)).toBe(0.1);
  });

  test('busy plate on a dark scrim stays readable enough or flags contrast', () => {
    const ratio = plateContrastRatio(analysis, 0.4, 0.8, 'dark');
    expect(ratio).toBeGreaterThan(3);
  });
});

describe('wallpaper prefs', () => {
  test('clamps dim and defaults disabled', () => {
    const prefs = normalizeWallpaperPrefs({
      ...DEFAULT_WALLPAPER_PREFS,
      enabled: true,
      id: '01999999-aaaa-7bbb-8ccc-ddddeeeeffff',
      dim: 9,
    });
    expect(prefs.dim).toBe(0.8);
    expect(prefs.kind).toBe('library');
    expect(normalizeWallpaperPrefs(undefined).enabled).toBe(false);
  });

  test('drops retired gradient skins and turns wallpaper off', () => {
    const prefs = normalizeWallpaperPrefs({
      enabled: true,
      kind: 'preset',
      id: 'dusk',
      lightId: 'dawn',
      darkId: 'midnight',
    });
    expect(prefs.enabled).toBe(false);
    expect(prefs.kind).toBe('library');
    expect(prefs.id).toBeNull();
    expect(prefs.lightId).toBeNull();
    expect(prefs.darkId).toBeNull();
  });

  test('explicit library id wins over leftover light/dark split ids', () => {
    const prefs = normalizeWallpaperPrefs({
      ...DEFAULT_WALLPAPER_PREFS,
      enabled: true,
      id: 'shared-wallpaper',
      lightId: 'light-wallpaper',
      darkId: 'dark-wallpaper',
    });
    expect(resolvedWallpaperId(prefs, 'light')).toBe('shared-wallpaper');
    expect(resolvedWallpaperId(prefs, 'dark')).toBe('shared-wallpaper');
  });

  test('falls back to scheme-specific ids when shared id is empty', () => {
    const prefs = normalizeWallpaperPrefs({
      ...DEFAULT_WALLPAPER_PREFS,
      enabled: true,
      id: null,
      lightId: 'light-wallpaper',
      darkId: 'dark-wallpaper',
    });
    expect(resolvedWallpaperId(prefs, 'light')).toBe('light-wallpaper');
    expect(resolvedWallpaperId(prefs, 'dark')).toBe('dark-wallpaper');
  });
});
