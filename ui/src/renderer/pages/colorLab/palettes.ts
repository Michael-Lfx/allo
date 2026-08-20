import type { CSSProperties } from 'react';

export const COLOR_LAB_IDS = ['now', 'forest', 'cobalt', 'inkTan', 'olive'] as const;

export type ColorLabId = (typeof COLOR_LAB_IDS)[number];
export type ColorLabMode = 'light' | 'dark';

export type ColorLabTokens = {
  canvas: string;
  panel: string;
  rail: string;
  railText: string;
  railMuted: string;
  railActive: string;
  session: string;
  raised: string;
  border: string;
  text: string;
  text2: string;
  text3: string;
  accent: string;
  accentFg: string;
  accentMuted: string;
  userBubble: string;
  composer: string;
  fill: string;
  shadow: string;
  titlebar: string;
};

type PalettePair = Record<ColorLabMode, ColorLabTokens>;

const now: PalettePair = {
  light: {
    canvas: '#ffffff',
    panel: '#fafafa',
    rail: '#f4f4f3',
    railText: '#191919',
    railMuted: '#737373',
    railActive: '#e5e5e4',
    session: '#f4f4f3',
    raised: '#ffffff',
    border: '#e7e7e5',
    text: '#191919',
    text2: '#525252',
    text3: '#8a8a86',
    accent: '#171717',
    accentFg: '#fafafa',
    accentMuted: '#f0f0ee',
    userBubble: '#f0f0ee',
    composer: '#ffffff',
    fill: '#f6f6f5',
    shadow: '0 18px 40px rgba(23, 23, 23, 0.08)',
    titlebar: '#f4f4f3',
  },
  dark: {
    canvas: '#111110',
    panel: '#191918',
    rail: '#161615',
    railText: '#f5f5f4',
    railMuted: '#a3a3a0',
    railActive: '#262624',
    session: '#191918',
    raised: '#222220',
    border: '#2e2e2b',
    text: '#f5f5f4',
    text2: '#d4d4d0',
    text3: '#a3a3a0',
    accent: '#d4d4d4',
    accentFg: '#171717',
    accentMuted: '#262624',
    userBubble: '#262624',
    composer: '#191918',
    fill: '#1f1f1d',
    shadow: '0 18px 40px rgba(0, 0, 0, 0.42)',
    titlebar: '#161615',
  },
};

const forest: PalettePair = {
  light: {
    canvas: '#eef3ef',
    panel: '#f7faf8',
    rail: '#1a2b24',
    railText: '#e8f0ec',
    railMuted: '#9bb0a8',
    railActive: '#24362e',
    session: '#e7eee9',
    raised: '#ffffff',
    border: '#d2ddd6',
    text: '#14201c',
    text2: '#3d524a',
    text3: '#6b8178',
    accent: '#0f766e',
    accentFg: '#ffffff',
    accentMuted: '#d7ebe6',
    userBubble: '#d7ebe6',
    composer: '#ffffff',
    fill: '#e3ece7',
    shadow: '0 18px 40px rgba(20, 32, 28, 0.1)',
    titlebar: '#16241f',
  },
  dark: {
    canvas: '#0f1714',
    panel: '#15201c',
    rail: '#0c1411',
    railText: '#e4eee9',
    railMuted: '#8aa198',
    railActive: '#1c2c26',
    session: '#15201c',
    raised: '#1c2a24',
    border: '#2a3b34',
    text: '#e4eee9',
    text2: '#c5d4cd',
    text3: '#8aa198',
    accent: '#2dd4bf',
    accentFg: '#042f2e',
    accentMuted: '#1a3a34',
    userBubble: '#1a3a34',
    composer: '#15201c',
    fill: '#1a2621',
    shadow: '0 18px 40px rgba(0, 0, 0, 0.45)',
    titlebar: '#0c1411',
  },
};

const cobalt: PalettePair = {
  light: {
    canvas: '#f4f0e6',
    panel: '#faf6ec',
    rail: '#1b2744',
    railText: '#eef2fb',
    railMuted: '#9aa8c4',
    railActive: '#243356',
    session: '#efe9db',
    raised: '#fffdf8',
    border: '#e0d8c6',
    text: '#1c2333',
    text2: '#44506a',
    text3: '#6d7890',
    accent: '#1d4ed8',
    accentFg: '#ffffff',
    accentMuted: '#dbe4f8',
    userBubble: '#dbe4f8',
    composer: '#fffdf8',
    fill: '#ece6d6',
    shadow: '0 18px 40px rgba(28, 35, 51, 0.1)',
    titlebar: '#16203a',
  },
  dark: {
    canvas: '#12151c',
    panel: '#1a2030',
    rail: '#0e1218',
    railText: '#e8edf8',
    railMuted: '#9aa6c0',
    railActive: '#1c2740',
    session: '#1a2030',
    raised: '#222b40',
    border: '#2c3650',
    text: '#e8edf8',
    text2: '#c5cde0',
    text3: '#9aa6c0',
    accent: '#60a5fa',
    accentFg: '#0b1b3a',
    accentMuted: '#1e2d4a',
    userBubble: '#1e2d4a',
    composer: '#1a2030',
    fill: '#1c2436',
    shadow: '0 18px 40px rgba(0, 0, 0, 0.45)',
    titlebar: '#0e1218',
  },
};

const inkTan: PalettePair = {
  light: {
    canvas: '#f3eadc',
    panel: '#faf4ea',
    rail: '#171412',
    railText: '#f6eee4',
    railMuted: '#b7a89a',
    railActive: '#2a2420',
    session: '#eee3d2',
    raised: '#fffaf3',
    border: '#e0d1bc',
    text: '#1c1917',
    text2: '#5c5148',
    text3: '#8a7c70',
    accent: '#c2410c',
    accentFg: '#ffffff',
    accentMuted: '#f3d7c4',
    userBubble: '#f0d9c4',
    composer: '#fffaf3',
    fill: '#eadfcf',
    shadow: '0 18px 40px rgba(23, 20, 18, 0.12)',
    titlebar: '#171412',
  },
  dark: {
    canvas: '#1a1612',
    panel: '#221c16',
    rail: '#100e0c',
    railText: '#f3e6d4',
    railMuted: '#b9a48e',
    railActive: '#2c241c',
    session: '#221c16',
    raised: '#2c241c',
    border: '#3a3128',
    text: '#f3e6d4',
    text2: '#d7c4ae',
    text3: '#b9a48e',
    accent: '#fb923c',
    accentFg: '#2a1406',
    accentMuted: '#3a2718',
    userBubble: '#3a2718',
    composer: '#221c16',
    fill: '#261f18',
    shadow: '0 18px 40px rgba(0, 0, 0, 0.48)',
    titlebar: '#100e0c',
  },
};

const olive: PalettePair = {
  light: {
    canvas: '#eef0e4',
    panel: '#f7f8f0',
    rail: '#23261c',
    railText: '#eef0e4',
    railMuted: '#a8ad98',
    railActive: '#323628',
    session: '#e6e8da',
    raised: '#fcfdf6',
    border: '#d5d8c6',
    text: '#1c1f16',
    text2: '#4d5340',
    text3: '#767c68',
    accent: '#9f2d20',
    accentFg: '#ffffff',
    accentMuted: '#f0d7d2',
    userBubble: '#f0d7d2',
    composer: '#fcfdf6',
    fill: '#e4e6d6',
    shadow: '0 18px 40px rgba(28, 31, 22, 0.1)',
    titlebar: '#1c1f16',
  },
  dark: {
    canvas: '#14160f',
    panel: '#1c1f16',
    rail: '#10120c',
    railText: '#eef0e4',
    railMuted: '#a8ad98',
    railActive: '#2a2e20',
    session: '#1c1f16',
    raised: '#272b1e',
    border: '#34382a',
    text: '#eef0e4',
    text2: '#cfd3bf',
    text3: '#a8ad98',
    accent: '#e85d4c',
    accentFg: '#2a0c08',
    accentMuted: '#3a201c',
    userBubble: '#3a201c',
    composer: '#1c1f16',
    fill: '#202318',
    shadow: '0 18px 40px rgba(0, 0, 0, 0.46)',
    titlebar: '#10120c',
  },
};

export const getColorLabTokens = (id: ColorLabId, mode: ColorLabMode): ColorLabTokens => {
  switch (id) {
    case 'now':
      return now[mode];
    case 'forest':
      return forest[mode];
    case 'cobalt':
      return cobalt[mode];
    case 'inkTan':
      return inkTan[mode];
    case 'olive':
      return olive[mode];
    default: {
      const exhaustive: never = id;
      throw new Error(`Unhandled color lab id: ${exhaustive}`);
    }
  }
};

export const tokensToStyle = (tokens: ColorLabTokens): CSSProperties =>
  ({
    '--lab-canvas': tokens.canvas,
    '--lab-panel': tokens.panel,
    '--lab-rail': tokens.rail,
    '--lab-rail-text': tokens.railText,
    '--lab-rail-muted': tokens.railMuted,
    '--lab-rail-active': tokens.railActive,
    '--lab-session': tokens.session,
    '--lab-raised': tokens.raised,
    '--lab-border': tokens.border,
    '--lab-text': tokens.text,
    '--lab-text-2': tokens.text2,
    '--lab-text-3': tokens.text3,
    '--lab-accent': tokens.accent,
    '--lab-accent-fg': tokens.accentFg,
    '--lab-accent-muted': tokens.accentMuted,
    '--lab-user-bubble': tokens.userBubble,
    '--lab-composer': tokens.composer,
    '--lab-fill': tokens.fill,
    '--lab-shadow': tokens.shadow,
    '--lab-titlebar': tokens.titlebar,
  }) as CSSProperties;
