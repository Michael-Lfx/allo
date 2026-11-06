/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

/**
 * Home template gallery: visual themes. A theme is one ppt-master visual style
 * (`references/visual-styles/<id>.md`, which deliberately carries no colors)
 * plus a fixed palette and a typography character. Both travel in the
 * `[ppt-master]` header as the user's explicit look choice; ppt-master's
 * Quick and Default routes honor user color anchors and a preset style id.
 */

/** First-column ids of `references/visual-styles/_index.md`. */
export type SlideVisualStyle =
  | 'swiss-minimal'
  | 'soft-rounded'
  | 'glassmorphism'
  | 'dark-tech'
  | 'blueprint'
  | 'editorial'
  | 'photo-editorial'
  | 'data-journalism'
  | 'brutalist'
  | 'memphis'
  | 'zine'
  | 'vintage-poster'
  | 'paper-cut'
  | 'sketch-notes'
  | 'ink-notes'
  | 'chalkboard'
  | 'ink-wash'
  | 'pixel-art';

export type SlideTypeface = 'sans' | 'serif' | 'brush' | 'hand' | 'mono' | 'display';

export interface SlidePalette {
  /** Page background; with `backgroundTo` it is a gradient. */
  background: string;
  backgroundTo?: string;
  text: string;
  primary: string;
  accent: string;
  /** Optional third color for multi-color styles. */
  extra?: string;
}

export interface SlideTheme {
  visualStyle: SlideVisualStyle;
  palette: SlidePalette;
  typeface: SlideTypeface;
}

export const SLIDE_THEME_IDS = [
  'swiss',
  'navy',
  'frost',
  'editorial',
  'midnight',
  'aurora',
  'data',
  'inkwash',
  'sketch',
  'chalk',
  'blueprint',
  'pictorial',
  'papercut',
  'retro',
  'pop',
  'riso',
  'newsprint',
  'inknotes',
  'cream',
  'pixel',
] as const;
export type SlideThemeId = (typeof SLIDE_THEME_IDS)[number];

export const SLIDE_THEMES: Record<SlideThemeId, SlideTheme> = {
  swiss: {
    visualStyle: 'swiss-minimal',
    palette: { background: '#FFFFFF', text: '#111111', primary: '#111111', accent: '#E63946' },
    typeface: 'sans',
  },
  navy: {
    visualStyle: 'swiss-minimal',
    palette: { background: '#0A2342', text: '#FFFFFF', primary: '#8FB8DE', accent: '#2CA58D' },
    typeface: 'sans',
  },
  frost: {
    visualStyle: 'soft-rounded',
    palette: { background: '#EEF4FA', text: '#1F3B57', primary: '#4A90D9', accent: '#9AD1F5' },
    typeface: 'sans',
  },
  editorial: {
    visualStyle: 'editorial',
    palette: { background: '#FBF9F4', text: '#1A1A1A', primary: '#1A1A1A', accent: '#C8102E' },
    typeface: 'serif',
  },
  midnight: {
    visualStyle: 'dark-tech',
    palette: { background: '#0B0F1A', text: '#E6EAF2', primary: '#3B82F6', accent: '#00E0B8' },
    typeface: 'sans',
  },
  aurora: {
    visualStyle: 'glassmorphism',
    palette: { background: '#1C2A6B', backgroundTo: '#7B3FD1', text: '#FFFFFF', primary: '#9CC3FF', accent: '#7DF9FF' },
    typeface: 'sans',
  },
  data: {
    visualStyle: 'data-journalism',
    palette: { background: '#FFFFFF', text: '#222222', primary: '#0072B2', accent: '#D55E00' },
    typeface: 'sans',
  },
  inkwash: {
    visualStyle: 'ink-wash',
    palette: { background: '#F4EFE4', text: '#1F1F1F', primary: '#3A3A3A', accent: '#B22222' },
    typeface: 'brush',
  },
  sketch: {
    visualStyle: 'sketch-notes',
    palette: { background: '#FBF6EC', text: '#3A3229', primary: '#F4A261', accent: '#8ECAE6' },
    typeface: 'hand',
  },
  chalk: {
    visualStyle: 'chalkboard',
    palette: { background: '#24332B', text: '#F1F1EA', primary: '#F7D774', accent: '#8FD3C7' },
    typeface: 'hand',
  },
  blueprint: {
    visualStyle: 'blueprint',
    palette: { background: '#0D2B52', text: '#E3EEFF', primary: '#8EC5FF', accent: '#FFB703' },
    typeface: 'mono',
  },
  pictorial: {
    visualStyle: 'photo-editorial',
    palette: { background: '#2B2622', text: '#FFFFFF', primary: '#C9A27E', accent: '#F5C518' },
    typeface: 'serif',
  },
  papercut: {
    visualStyle: 'paper-cut',
    palette: { background: '#FFF1E6', text: '#5B3A29', primary: '#F4845F', accent: '#F7B267' },
    typeface: 'sans',
  },
  retro: {
    visualStyle: 'vintage-poster',
    palette: { background: '#F1E4C9', text: '#2E2A24', primary: '#D1495B', accent: '#EDAE49', extra: '#00798C' },
    typeface: 'display',
  },
  pop: {
    visualStyle: 'memphis',
    palette: { background: '#FFF5E1', text: '#1B1B1B', primary: '#FF5A5F', accent: '#00A6A6', extra: '#FFD23F' },
    typeface: 'display',
  },
  riso: {
    visualStyle: 'zine',
    palette: { background: '#F6F1E7', text: '#1D3557', primary: '#1D3557', accent: '#FF477E' },
    typeface: 'display',
  },
  newsprint: {
    visualStyle: 'brutalist',
    palette: { background: '#F1EFE7', text: '#000000', primary: '#000000', accent: '#FF3B00' },
    typeface: 'display',
  },
  inknotes: {
    visualStyle: 'ink-notes',
    palette: { background: '#FCFCFA', text: '#141414', primary: '#141414', accent: '#E4572E' },
    typeface: 'hand',
  },
  cream: {
    visualStyle: 'soft-rounded',
    palette: { background: '#FFF8F0', text: '#3D3A35', primary: '#F28C6A', accent: '#FFD6A5' },
    typeface: 'sans',
  },
  pixel: {
    visualStyle: 'pixel-art',
    palette: { background: '#1D1F33', text: '#FFFFFF', primary: '#FF6B6B', accent: '#4ECDC4', extra: '#FFE66D' },
    typeface: 'mono',
  },
};

/** Model-facing typography character (ppt-master picks the exact families). */
const TYPEFACE_DIRECTIVE: Record<SlideTypeface, string> = {
  sans: 'clean sans titles and body',
  serif: 'serif titles, clean sans body',
  brush: 'calligraphic brush titles, clean sans body',
  hand: 'hand-lettered titles, rounded sans body',
  mono: 'monospace technical titles, clean sans body',
  display: 'heavy display titles, sans body',
};

/** `theme=ink-wash…; ` header fragment for one theme. */
export function themeDirective(id: SlideThemeId): string {
  const { visualStyle, palette, typeface } = SLIDE_THEMES[id];
  const background = palette.backgroundTo ? `${palette.background}→${palette.backgroundTo}` : palette.background;
  const extra = palette.extra ? `, extra ${palette.extra}` : '';
  return (
    `look=${id}: visual_style=${visualStyle} (references/visual-styles/${visualStyle}.md); ` +
    `colors background ${background}, text ${palette.text}, primary ${palette.primary}, accent ${palette.accent}${extra}; ` +
    `typography ${TYPEFACE_DIRECTIVE[typeface]} (user-chosen look, keep it); `
  );
}
