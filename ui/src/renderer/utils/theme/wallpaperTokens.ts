import type { WallpaperAnalysis, WallpaperPrefs } from '@/common/types/appearance';

export const WALLPAPER_TOKENS_STYLE_ID = 'user-wallpaper-tokens';

/** Lives in the late token sheet so ambiance presets cannot re-opaque the chrome. */
export const WALLPAPER_SURFACE_PUNCH_CSS = [
  `html[data-wallpaper='on'] .layout-content,`,
  `html[data-wallpaper='on'] .layout-content.bg-base,`,
  `html[data-wallpaper='on'] .layout-content .arco-layout,`,
  `html[data-wallpaper='on'] .layout-content .arco-layout-content,`,
  `html[data-wallpaper='on'] .layout-content > *,`,
  `html[data-wallpaper='on'] .oc-root:not(.oc-portal-host),`,
  `html[data-wallpaper='on'] .oc-shell,`,
  `html[data-wallpaper='on'] .oc-canvas:not(.oc-portal-host),`,
  `html[data-wallpaper='on'] #canvas-main,`,
  `html[data-wallpaper='on'] [data-infinite-canvas],`,
  `html[data-wallpaper='on'] [data-canvas-refresh-shell],`,
  `html[data-wallpaper='on'] .oc-canvas:not(.oc-portal-host) > main,`,
  `html[data-wallpaper='on'] .app-page-shell,`,
  `html[data-wallpaper='on'] .conversation-column-slide,`,
  `html[data-wallpaper='on'] .conversation-column-viewport,`,
  `html[data-wallpaper='on'] .conversation-column-pane,`,
  `html[data-wallpaper='on'] .chat-layout-chrome,`,
  `html[data-wallpaper='on'] .chat-layout-right-sider,`,
  `html[data-wallpaper='on'] .content-sider,`,
  `html[data-wallpaper='on'] .workspace-tool-rail,`,
  `html[data-wallpaper='on'] .workspace-panel-header,`,
  `html[data-wallpaper='on'] .preview-panel,`,
  `html[data-wallpaper='on'] .preview-panel-shell,`,
  `html[data-wallpaper='on'] .preview-tabs,`,
  `html[data-wallpaper='on'] .chat-workspace,`,
  `html[data-wallpaper='on'] .chat-workspace .arco-tree,`,
  `html[data-wallpaper='on'] .chat-workspace .arco-tree-list,`,
  `html[data-wallpaper='on'] .file-change-group,`,
  `html[data-wallpaper='on'] .file-change-group-header,`,
  `html[data-wallpaper='on'] .d2h-wrapper,`,
  `html[data-wallpaper='on'] .d2h-file-wrapper,`,
  `html[data-wallpaper='on'] .d2h-file-header,`,
  `html[data-wallpaper='on'] .d2h-file-diff,`,
  `html[data-wallpaper='on'] .d2h-files-diff,`,
  `html[data-wallpaper='on'] .d2h-diff-table,`,
  `html[data-wallpaper='on'] .d2h-diff-tbody,`,
  `html[data-wallpaper='on'] .collapsible-content__mask,`,
  `html[data-wallpaper='on'] .xterm,`,
  `html[data-wallpaper='on'] .xterm-viewport,`,
  `html[data-wallpaper='on'] .xterm-screen,`,
  `html[data-wallpaper='on'] .xterm-rows,`,
  `html[data-wallpaper='on'] .layout-sider,`,
  `html[data-wallpaper='on'] .layout-sider.arco-layout-sider,`,
  `html[data-wallpaper='on'] .layout-sider-header,`,
  `html[data-wallpaper='on'] .layout-sider-content,`,
  `html[data-wallpaper='on'] .layout-sider .arco-layout-sider-children,`,
  `html[data-wallpaper='on'] .app-titlebar,`,
  `html[data-wallpaper='on'] .app-titlebar--mobile-conversation,`,
  `html[data-wallpaper='on'] .chat-layout-header,`,
  `html[data-wallpaper='on'] .chat-layout-header--glass,`,
  `html[data-wallpaper='on'] .oc-portal-host {`,
  `  background: transparent !important;`,
  `  background-color: transparent !important;`,
  `  background-image: none !important;`,
  `  backdrop-filter: none !important;`,
  `  -webkit-backdrop-filter: none !important;`,
  `}`,
  `html[data-wallpaper='on'] .layout-sider,`,
  `html[data-wallpaper='on'] .layout-sider.arco-layout-sider,`,
  `html[data-wallpaper='on'] .app-titlebar {`,
  `  box-shadow: none !important;`,
  `}`,
].join('\n');

export interface WallpaperTokenInput {
  enabled: boolean;
  prefs: WallpaperPrefs;
  analysis: WallpaperAnalysis | null;
  scheme: 'light' | 'dark';
}

const rgb = (triplet: number[]): string => `${triplet[0] ?? 0}, ${triplet[1] ?? 0}, ${triplet[2] ?? 0}`;

const hexToRgb = (hex: string): [number, number, number] => {
  const raw = hex.replace('#', '');
  if (raw.length !== 6) return [32, 32, 32];
  return [
    Number.parseInt(raw.slice(0, 2), 16),
    Number.parseInt(raw.slice(2, 4), 16),
    Number.parseInt(raw.slice(4, 6), 16),
  ];
};

export const resolveWallpaperDim = (prefs: WallpaperPrefs, analysis: WallpaperAnalysis | null): number => {
  if (prefs.dim === 'auto') {
    return analysis ? Math.min(0.8, Math.max(0, analysis.recommendedDim)) : 0.32;
  }
  return Math.min(0.8, Math.max(0, prefs.dim));
};

export const resolveWallpaperBlur = (prefs: WallpaperPrefs, analysis: WallpaperAnalysis | null): number => {
  if (prefs.blur === 'auto') {
    return analysis ? Math.min(24, Math.max(0, analysis.recommendedBlurPx)) : 0;
  }
  return Math.min(24, Math.max(0, prefs.blur));
};

/**
 * WCAG-ish plate contrast against a scrimmed seed. Used for the 换肤 warning chip.
 * Architecture names APCA Lc 60; this ratio ≥ 4.5 is the structural equivalent
 * we can compute without a third-party APCA package.
 */
export const plateContrastRatio = (
  analysis: WallpaperAnalysis | null,
  dim: number,
  plateAlpha: number,
  scheme: 'light' | 'dark'
): number => {
  const seed = hexToRgb(analysis?.seedHex ?? '#808080');
  const scrim = scheme === 'light' ? [255, 255, 255] : [0, 0, 0];
  const mixed = seed.map((channel, i) => channel * (1 - dim) + (scrim[i] ?? 0) * dim);
  const plate = scheme === 'light' ? [255, 255, 255] : [28, 28, 30];
  const surface = mixed.map((channel, i) => (plate[i] ?? 0) * plateAlpha + channel * (1 - plateAlpha));
  const text = scheme === 'light' ? [29, 29, 31] : [245, 245, 247];
  const lum = (rgbArr: number[]) => {
    const chan = (value: number) => {
      const unit = value / 255;
      return unit <= 0.04045 ? unit / 12.92 : ((unit + 0.055) / 1.055) ** 2.4;
    };
    return 0.2126 * chan(rgbArr[0] ?? 0) + 0.7152 * chan(rgbArr[1] ?? 0) + 0.0722 * chan(rgbArr[2] ?? 0);
  };
  const [lighter, darker] = [lum(text), lum(surface)].sort((a, b) => b - a);
  return (lighter + 0.05) / (darker + 0.05);
};

export const buildWallpaperTokenCss = ({ enabled, prefs, analysis, scheme }: WallpaperTokenInput): string => {
  if (!enabled || !analysis) {
    return '';
  }
  const dim = resolveWallpaperDim(prefs, analysis);
  const blur = resolveWallpaperBlur(prefs, analysis);
  const plateAlpha = Math.min(0.86, Math.max(0.55, analysis.recommendedPlateAlpha));
  const chromeAlpha = 0;
  const scrimColor = scheme === 'light' ? '255, 255, 255' : '0, 0, 0';
  const assistantPlate = scheme === 'light' ? '255, 255, 255' : '28, 28, 32';
  const seed = hexToRgb(analysis.seedHex);
  const lines = [
    `html[data-wallpaper='on'] {`,
    `  --wallpaper-seed: ${analysis.seedHex};`,
    `  --wallpaper-seed-rgb: ${seed[0]}, ${seed[1]}, ${seed[2]};`,
    `  --wallpaper-dim: ${dim};`,
    `  --wallpaper-blur: ${blur}px;`,
    `  --wallpaper-scrim-color: rgb(${scrimColor});`,
    `  --wallpaper-chrome-alpha: ${chromeAlpha};`,
    `  --wallpaper-bubble-alpha: ${plateAlpha};`,
    `  --wallpaper-plate-blur: 0px;`,
    `  --wallpaper-assistant-plate: rgb(${assistantPlate});`,
    `  --wallpaper-fit: ${prefs.fit};`,
    `  --wallpaper-position: ${prefs.position};`,
    `  --terminal-surface-bg: transparent;`,
    `}`,
  ];
  if (prefs.harmonizeAccent && analysis.primaryScale.length >= 7) {
    const primary = analysis.primaryRgb;
    lines.push(`html[data-wallpaper='on'] {`);
    lines.push(`  --color-primary: rgb(${rgb(primary)});`);
    lines.push(`  --primary-rgb: ${rgb(primary)};`);
    analysis.primaryScale.slice(0, 7).forEach((triplet, index) => {
      lines.push(`  --primary-${index + 1}: ${rgb(triplet)};`);
    });
    lines.push(`}`);
  }
  lines.push(WALLPAPER_SURFACE_PUNCH_CSS);
  return lines.join('\n');
};

export interface WallpaperFoucCache {
  enabled: boolean;
  seed: string;
  dim: number;
}

export const wallpaperFoucCache = (input: WallpaperTokenInput): WallpaperFoucCache => ({
  enabled: input.enabled && Boolean(input.analysis),
  seed: input.analysis?.seedHex ?? '#1A1A1A',
  dim: resolveWallpaperDim(input.prefs, input.analysis),
});
