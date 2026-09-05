/**
 * One Look catalog for every video-generation surface.
 * Model prompts stay in visualStylePresets; project specs stay compiled from canvas axes.
 */
import {
  lookbookCanvasStylePresets,
  resolveCanvasStylePreset,
  styleCoverFromSelection,
  type CanvasStyleCover,
  type CanvasStylePreset,
  type ProjectStyleSelection,
} from '@oc/lib/canvas/canvas-style-system';
import type { CreationSkillId } from '../home/types';
import {
  VISUAL_STYLE_CATEGORIES,
  VISUAL_STYLE_PRESETS,
  type VisualStyleCategory,
  type VisualStylePreset,
} from '../visualStylePresets';
import { lookCoverImage } from './lookCovers';

export const FEATURED_LOOK_IDS = ['cinematic', 'anime', 'cyberpunk', 'inkWash'] as const satisfies readonly CreationSkillId[];

export const VIMAX_KEY_TO_CANVAS_PRESET: Record<string, string> = {
  cinematic: 'urban-live-action',
  documentary: 'real-life-documentary',
  urbanRomance: 'urban-live-action',
  campusYouth: 'campus-youth',
  kDrama: 'urban-live-action',
  hkCinema: 'retro-hong-kong',
  republicanEra: 'retro-hong-kong',
  vintageFilm: 'retro-hong-kong',
  xianxia: 'period-live-action',
  wuxia: 'period-live-action',
  thrillerTeal: 'suspense-noir',
  manhua3d: 'fantasy-3d',
  anime: 'chinese-2d',
  animeCel: 'chinese-2d',
  donghua: 'chinese-2d',
  inkWash: 'ink-narrative',
  claymation: 'clay-stop-motion',
  stopMotion: 'clay-stop-motion',
  illustration: 'storybook-fantasy',
  watercolor: 'storybook-fantasy',
  comic: 'comic-pop',
  cyberpunk: 'cyberpunk-neon',
  sciFiClean: 'future-tech',
  noir: 'black-white-noir',
  dreamlike: 'surreal-dream',
  pixar3d: 'three-d-cartoon',
  americanCartoon: 'three-d-cartoon',
  postApocalyptic: 'v2-wasteland--dark--live-action--realistic',
};

export const CREATION_SKILL_CANVAS_PRESET: Record<CreationSkillId, string> = {
  cinematic: VIMAX_KEY_TO_CANVAS_PRESET.cinematic,
  anime: VIMAX_KEY_TO_CANVAS_PRESET.anime,
  cyberpunk: VIMAX_KEY_TO_CANVAS_PRESET.cyberpunk,
  inkWash: VIMAX_KEY_TO_CANVAS_PRESET.inkWash,
};

const CATEGORY_COVER: Record<VisualStyleCategory, CanvasStyleCover> = {
  liveAction: { from: '#243848', via: '#8a8070', to: '#d6c6a8' },
  genreMood: { from: '#121018', via: '#c43b7a', to: '#d4a45c' },
  animation: { from: '#243848', via: '#7aa3b8', to: '#efe4c4' },
  illustration: { from: '#5f8454', via: '#d6c6a8', to: '#efe4c4' },
  crafted: { from: '#161e26', via: '#efc46a', to: '#c5ced6' },
};

const LOOKBOOK_CATEGORY: Record<string, VisualStyleCategory> = {
  真人实拍: 'liveAction',
  年代影像: 'liveAction',
  风格化实拍: 'liveAction',
  科幻影像: 'genreMood',
  二维动画: 'animation',
  风格化动画: 'animation',
  三维动画: 'animation',
  手工媒介: 'crafted',
  插画媒介: 'illustration',
};

export type UnifiedLook = {
  id: string;
  vimaxKey?: string;
  canvasPresetId: string;
  featured: boolean;
  category: VisualStyleCategory;
  labelKey: string;
  defaultLabel: string;
  descriptionKey?: string;
  defaultDescription?: string;
  modelPrompt: string;
  cover: CanvasStyleCover;
  axes?: ProjectStyleSelection;
};

function withCoverImage(cover: CanvasStyleCover, id: string): CanvasStyleCover {
  return { ...cover, image: lookCoverImage(id) };
}

function coverFor(canvasId: string, category: VisualStyleCategory, axes?: ProjectStyleSelection): CanvasStyleCover {
  if (axes) return styleCoverFromSelection(axes);
  const canvas = resolveCanvasStylePreset(canvasId);
  if (canvas?.cover) return canvas.cover;
  return CATEGORY_COVER[category];
}

function fromVimax(preset: VisualStylePreset): UnifiedLook {
  const canvasPresetId = VIMAX_KEY_TO_CANVAS_PRESET[preset.key] ?? `look:${preset.key}`;
  const canvas = resolveCanvasStylePreset(canvasPresetId);
  return {
    id: preset.key,
    vimaxKey: preset.key,
    canvasPresetId,
    featured: FEATURED_LOOK_IDS.includes(preset.key as CreationSkillId),
    category: preset.category,
    labelKey: preset.labelKey,
    defaultLabel: preset.defaultLabel,
    modelPrompt: preset.prompt,
    cover: withCoverImage(coverFor(canvasPresetId, preset.category, canvas?.selection), preset.key),
    axes: canvas?.selection,
  };
}

function fromLookbook(preset: CanvasStylePreset): UnifiedLook {
  const category = LOOKBOOK_CATEGORY[preset.category] ?? 'liveAction';
  return {
    id: preset.id,
    canvasPresetId: preset.id,
    featured: false,
    category,
    labelKey: `videoCanvas.stylePicker.legacy.${preset.id}.title`,
    defaultLabel: preset.title,
    descriptionKey: `videoCanvas.stylePicker.legacy.${preset.id}.description`,
    defaultDescription: preset.description,
    modelPrompt: preset.prompt,
    cover: withCoverImage(preset.cover, preset.id),
    axes: preset.selection,
  };
}

const mappedCanvasIds = new Set(Object.values(VIMAX_KEY_TO_CANVAS_PRESET));

export const HOME_LOOKS: readonly UnifiedLook[] = [
  ...VISUAL_STYLE_PRESETS.map(fromVimax),
  ...lookbookCanvasStylePresets.filter((preset) => !mappedCanvasIds.has(preset.id)).map(fromLookbook),
];

export const CANVAS_LOOKS: readonly UnifiedLook[] = lookbookCanvasStylePresets.map(fromLookbook);

export const lookById = new Map(HOME_LOOKS.map((look) => [look.id, look]));

export const lookByPrompt = new Map<string, UnifiedLook>();
export const lookByCanvasId = new Map<string, UnifiedLook>();
for (const look of HOME_LOOKS) {
  if (!lookByPrompt.has(look.modelPrompt)) lookByPrompt.set(look.modelPrompt, look);
  if (!lookByCanvasId.has(look.canvasPresetId)) lookByCanvasId.set(look.canvasPresetId, look);
}

export function featuredLooks(): UnifiedLook[] {
  return FEATURED_LOOK_IDS.map((id) => lookById.get(id)).filter((look): look is UnifiedLook => Boolean(look));
}

export function lookToCanvasPreset(look: UnifiedLook): CanvasStylePreset {
  const resolved = look.canvasPresetId.startsWith('look:')
    ? undefined
    : resolveCanvasStylePreset(look.canvasPresetId);
  if (resolved) return { ...resolved, cover: look.cover };
  return {
    id: look.canvasPresetId,
    title: look.defaultLabel,
    category: look.defaultLabel,
    description: look.defaultDescription || look.defaultLabel,
    tags: look.defaultDescription ? [look.defaultLabel] : [],
    prompt: look.modelPrompt,
    cover: look.cover,
    selection: look.axes,
  };
}

export function composeClipPrompt(userPrompt: string, stylePrompt: string): string {
  const look = stylePrompt.trim();
  const text = userPrompt.trim();
  if (!look) return text;
  if (!text) return look;
  return text.includes(look) ? text : `${look}\n\n${text}`;
}

export function groupLooks(
  looks: readonly UnifiedLook[],
  category: 'all' | VisualStyleCategory,
): Array<{ id: VisualStyleCategory; looks: UnifiedLook[] }> {
  return VISUAL_STYLE_CATEGORIES.flatMap((meta) => {
    if (category !== 'all' && meta.id !== category) return [];
    const grouped = looks.filter((look) => look.category === meta.id);
    return grouped.length ? [{ id: meta.id, looks: grouped }] : [];
  });
}
