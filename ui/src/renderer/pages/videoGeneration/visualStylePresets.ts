/**
 * Curated visual styles for nomi-vimax cast & film prompts.
 *
 * Catalog shaped by mainstream video tools (Runway / Kling / Pika / Higgsfield /
 * Vidu / 即梦 / Midjourney): live-action film looks, genre moods, animation media,
 * illustration craft, and crafted / social aesthetics — not a flat 12-item list.
 *
 * Prompts are English (pipeline / image models consume them as-is).
 * Stylized keys intentionally include needles from `wants_stylized_non_photoreal`.
 */

export type VisualStyleCategory =
  | 'liveAction'
  | 'genreMood'
  | 'animation'
  | 'illustration'
  | 'crafted';

export interface VisualStylePreset {
  key: string;
  category: VisualStyleCategory;
  /** i18n key under `videoGeneration.workspace.source.stylePresets.*` */
  labelKey: string;
  /** Fallback label when i18n is missing. */
  defaultLabel: string;
  /** Prompt text stored on the session / sent to the backend. */
  prompt: string;
}

export interface VisualStyleCategoryMeta {
  id: VisualStyleCategory;
  /** i18n key under `videoGeneration.workspace.source.styleCategories.*` */
  labelKey: string;
  defaultLabel: string;
}

/** Display order for OptGroup headers. */
export const VISUAL_STYLE_CATEGORIES: readonly VisualStyleCategoryMeta[] = [
  {
    id: 'liveAction',
    labelKey: 'videoGeneration.workspace.source.styleCategories.liveAction',
    defaultLabel: '实拍电影',
  },
  {
    id: 'genreMood',
    labelKey: 'videoGeneration.workspace.source.styleCategories.genreMood',
    defaultLabel: '类型氛围',
  },
  {
    id: 'animation',
    labelKey: 'videoGeneration.workspace.source.styleCategories.animation',
    defaultLabel: '动画三维',
  },
  {
    id: 'illustration',
    labelKey: 'videoGeneration.workspace.source.styleCategories.illustration',
    defaultLabel: '插画手绘',
  },
  {
    id: 'crafted',
    labelKey: 'videoGeneration.workspace.source.styleCategories.crafted',
    defaultLabel: '潮流特效',
  },
] as const;

/** Matches backend `DEFAULT_VISUAL_STYLE` in nomi-vimax planning. */
export const DEFAULT_VISUAL_STYLE_PROMPT =
  'cinematic film look, believable designed characters, natural wardrobe and lighting, clean healthy facial skin with clear readable features';

export const VISUAL_STYLE_PRESETS: readonly VisualStylePreset[] = [
  // —— Live-action / film craft (Runway, Kling, 海螺 cinematic) ——
  {
    key: 'cinematic',
    category: 'liveAction',
    labelKey: 'videoGeneration.workspace.source.stylePresets.cinematic',
    defaultLabel: '电影写实',
    prompt: DEFAULT_VISUAL_STYLE_PROMPT,
  },
  {
    key: 'anamorphicBlockbuster',
    category: 'liveAction',
    labelKey: 'videoGeneration.workspace.source.stylePresets.anamorphicBlockbuster',
    defaultLabel: '宽银幕大片',
    prompt:
      'Hollywood blockbuster anamorphic cinema, 2.39 widescreen framing, teal-and-orange grade, motivated practicals with rim light, shallow depth of field, clean healthy facial skin with clear readable features',
  },
  {
    key: 'documentary',
    category: 'liveAction',
    labelKey: 'videoGeneration.workspace.source.stylePresets.documentary',
    defaultLabel: '纪录片',
    prompt:
      'documentary cinema look, natural ambient light, handheld intimacy, observational framing, clean healthy faces with clear features, authentic wardrobe and locations',
  },
  {
    key: 'vintageFilm',
    category: 'liveAction',
    labelKey: 'videoGeneration.workspace.source.stylePresets.vintageFilm',
    defaultLabel: '复古胶片',
    prompt:
      '35mm vintage film look, Kodak Portra color science, soft halation, fine grain, warm highlights, cinematic framing, clean healthy facial skin with clear readable features',
  },
  {
    key: 'editorial',
    category: 'liveAction',
    labelKey: 'videoGeneration.workspace.source.stylePresets.editorial',
    defaultLabel: '时尚大片',
    prompt:
      'high-fashion editorial photography look, sculpted beauty lighting, rich fabric texture, magazine-cover composition, polished color grade, clean healthy facial skin with clear readable features',
  },
  {
    key: 'commercial',
    category: 'liveAction',
    labelKey: 'videoGeneration.workspace.source.stylePresets.commercial',
    defaultLabel: '商业广告',
    prompt:
      'premium commercial advertising look, clean keyed lighting, crisp product-grade detail, shallow depth of field, polished color grade, clean healthy facial skin with clear features',
  },
  {
    key: 'indieHandheld',
    category: 'liveAction',
    labelKey: 'videoGeneration.workspace.source.stylePresets.indieHandheld',
    defaultLabel: '独立手持',
    prompt:
      'raw indie film aesthetic, handheld documentary camera, natural available light, low-contrast unpolished grade, intimate observational feel, clean healthy faces with clear features',
  },

  // —— Genre / mood (Kling director-emulation, Runway western/epic cues) ——
  {
    key: 'noir',
    category: 'genreMood',
    labelKey: 'videoGeneration.workspace.source.stylePresets.noir',
    defaultLabel: '黑色电影',
    prompt:
      'classic film noir, high-contrast black and white cinematography, dramatic chiaroscuro lighting, deep shadows, anamorphic bokeh, clean healthy faces with clear features',
  },
  {
    key: 'cyberpunk',
    category: 'genreMood',
    labelKey: 'videoGeneration.workspace.source.stylePresets.cyberpunk',
    defaultLabel: '赛博朋克',
    prompt:
      'cyberpunk neo-noir night city, neon rim light, rain-slick streets, teal-and-magenta grade, volumetric haze, cinematic composition, clean healthy facial skin with clear readable features',
  },
  {
    key: 'westernEpic',
    category: 'genreMood',
    labelKey: 'videoGeneration.workspace.source.stylePresets.westernEpic',
    defaultLabel: '西部史诗',
    prompt:
      'western epic cinema, dusty golden-hour backlight, high contrast silhouettes, warm amber and deep orange grade, atmospheric haze, cinematic framing, clean healthy faces with clear features',
  },
  {
    key: 'romanticGolden',
    category: 'genreMood',
    labelKey: 'videoGeneration.workspace.source.stylePresets.romanticGolden',
    defaultLabel: '浪漫金辉',
    prompt:
      'classical romantic cinema, soft pink-and-golden hour light, gentle bloom, intimate close framing, warm nostalgic grade, clean healthy facial skin with clear readable features',
  },
  {
    key: 'horrorGothic',
    category: 'genreMood',
    labelKey: 'videoGeneration.workspace.source.stylePresets.horrorGothic',
    defaultLabel: '哥特恐怖',
    prompt:
      'gothic horror cinema, cold desaturated palette, practical candle and moonlight, deep negative fill, unsettling slow camera, cinematic tension, clean healthy faces with clear readable features',
  },
  {
    key: 'sciFiClean',
    category: 'genreMood',
    labelKey: 'videoGeneration.workspace.source.stylePresets.sciFiClean',
    defaultLabel: '科幻冷调',
    prompt:
      'clean sci-fi cinema, cool cyan-silver grade, soft LED panels and practical screens, precise geometric sets, shallow depth of field, clean healthy facial skin with clear readable features',
  },

  // —— Animation / 3D (Pika, Higgsfield, Vidu, Pixar/Ghibli) ——
  {
    key: 'anime',
    category: 'animation',
    labelKey: 'videoGeneration.workspace.source.stylePresets.anime',
    defaultLabel: '日式动画',
    prompt:
      'theatrical anime / animated-film character design, clear volume and soft painted shading, detailed hair strands and fabric folds, rich wardrobe materials, storybook colors — NOT flat paper-doll cel cutout, NOT photoreal live-action',
  },
  {
    key: 'ghibli',
    category: 'animation',
    labelKey: 'videoGeneration.workspace.source.stylePresets.ghibli',
    defaultLabel: '吉卜力风',
    prompt:
      'Studio Ghibli inspired hand-drawn animation, soft watercolor backgrounds, warm natural light, gentle character acting, painterly sky and foliage, cinematic anime composition — NOT photoreal live-action',
  },
  {
    key: 'donghua',
    category: 'animation',
    labelKey: 'videoGeneration.workspace.source.stylePresets.donghua',
    defaultLabel: '国风动效',
    prompt:
      'Chinese donghua / animated-film look, refined linework with soft cel shading, flowing fabric and hair detail, ink-inspired accents, cinematic anime staging — NOT flat paper-doll cutout, NOT photoreal live-action',
  },
  {
    key: 'pixar3d',
    category: 'animation',
    labelKey: 'videoGeneration.workspace.source.stylePresets.pixar3d',
    defaultLabel: '3D 动画电影',
    prompt:
      'Pixar / Disney style 3D animated film, appealing character design, subsurface skin, detailed cloth and hair, cinematic lighting and camera language, warm family-film color — NOT photoreal live-action',
  },
  {
    key: 'claymation',
    category: 'animation',
    labelKey: 'videoGeneration.workspace.source.stylePresets.claymation',
    defaultLabel: '黏土定格',
    prompt:
      'soft 3D claymation / stop-motion look, rounded plasticine forms, matte clay texture with subtle fingerprint detail, tactile miniature sets, studio soft light — NOT photoreal live-action',
  },
  {
    key: 'stopMotion',
    category: 'animation',
    labelKey: 'videoGeneration.workspace.source.stylePresets.stopMotion',
    defaultLabel: '定格偶戏',
    prompt:
      'premium stop-motion puppet animation, handcrafted fabric and felt textures, miniature practical sets, slight frame-step motion feel, warm tactile lighting — NOT photoreal live-action, NOT flat 2D cartoon',
  },

  // —— Illustration / craft (Midjourney, 即梦, Vidu) ——
  {
    key: 'illustration',
    category: 'illustration',
    labelKey: 'videoGeneration.workspace.source.stylePresets.illustration',
    defaultLabel: '绘本插画',
    prompt:
      'painted illustration style, detailed brushwork, storybook atmosphere, cinematic composition (not anime), expressive designed characters',
  },
  {
    key: 'watercolor',
    category: 'illustration',
    labelKey: 'videoGeneration.workspace.source.stylePresets.watercolor',
    defaultLabel: '水彩手绘',
    prompt:
      'soft watercolor hand-drawn illustration, translucent washes, paper texture, gentle edges, storybook palette, cinematic framing — NOT photoreal live-action',
  },
  {
    key: 'inkWash',
    category: 'illustration',
    labelKey: 'videoGeneration.workspace.source.stylePresets.inkWash',
    defaultLabel: '水墨写意',
    prompt:
      'Chinese ink-wash painting animation, expressive brush strokes, misty negative space, restrained ink palette with soft wet edges, poetic cinematic framing — NOT photoreal live-action',
  },
  {
    key: 'oilPaint',
    category: 'illustration',
    labelKey: 'videoGeneration.workspace.source.stylePresets.oilPaint',
    defaultLabel: '油画质感',
    prompt:
      'classical oil painting look, visible impasto brushwork, rich glazed color, Rembrandt-inspired lighting, cinematic portrait composition — NOT photoreal live-action photography',
  },
  {
    key: 'comic',
    category: 'illustration',
    labelKey: 'videoGeneration.workspace.source.stylePresets.comic',
    defaultLabel: '欧美漫画',
    prompt:
      'graphic novel / comic book style, bold ink linework, dramatic panel lighting, rich flat-to-cel color, cinematic comic composition — NOT photoreal live-action',
  },
  {
    key: 'webtoon',
    category: 'illustration',
    labelKey: 'videoGeneration.workspace.source.stylePresets.webtoon',
    defaultLabel: '韩漫竖屏',
    prompt:
      'Korean webtoon / manhwa illustration style, clean digital linework, soft gradient cel shading, expressive eyes, vertical-scroll friendly framing, polished comic color — NOT photoreal live-action',
  },

  // —— Crafted / social aesthetics (Pika Powers, Higgsfield, Vidu 盲盒) ——
  {
    key: 'pixelArt',
    category: 'crafted',
    labelKey: 'videoGeneration.workspace.source.stylePresets.pixelArt',
    defaultLabel: '像素复古',
    prompt:
      'premium pixel-art animation, deliberate low-resolution mosaic, limited retro palette, crisp pixel clusters, cinematic staging within pixel medium — NOT photoreal live-action, NOT smooth 3D',
  },
  {
    key: 'blindBox3d',
    category: 'crafted',
    labelKey: 'videoGeneration.workspace.source.stylePresets.blindBox3d',
    defaultLabel: '盲盒潮玩',
    prompt:
      'collectible blind-box / designer toy 3D look, chibi proportions, soft matte vinyl material, cute stylized faces, clean studio product lighting — NOT photoreal live-action',
  },
  {
    key: 'dreamlike',
    category: 'crafted',
    labelKey: 'videoGeneration.workspace.source.stylePresets.dreamlike',
    defaultLabel: '梦幻超现实',
    prompt:
      'dreamlike surreal cinema, soft ethereal bloom, floating particles, impossible gentle physics, pastel-mist color grade, poetic composition, clean healthy faces with clear readable features',
  },
  {
    key: 'vhsRetro',
    category: 'crafted',
    labelKey: 'videoGeneration.workspace.source.stylePresets.vhsRetro',
    defaultLabel: 'VHS 录像带',
    prompt:
      'vintage VHS home-video look, soft tracking noise, slight chromatic aberration, muted 90s color cast, CRT softness, nostalgic handheld framing, clean healthy faces with clear features',
  },
  {
    key: 'isometric',
    category: 'crafted',
    labelKey: 'videoGeneration.workspace.source.stylePresets.isometric',
    defaultLabel: '等距微缩',
    prompt:
      'clean isometric 3D miniature diorama, 2:1 axonometric view, soft ambient occlusion, toy-scale sets and characters, even studio light — NOT photoreal perspective, NOT flat paper cutout',
  },
] as const;

export function findVisualStylePreset(style: string | undefined | null): VisualStylePreset | undefined {
  const trimmed = (style ?? '').trim();
  if (!trimmed) return VISUAL_STYLE_PRESETS[0];
  return VISUAL_STYLE_PRESETS.find((preset) => preset.prompt === trimmed);
}

/** Select value for a stored style string (empty → cinematic default). */
export function visualStyleSelectValue(style: string | undefined | null): string {
  const trimmed = (style ?? '').trim();
  if (!trimmed) return 'cinematic';
  return findVisualStylePreset(trimmed)?.key ?? '__custom__';
}

export function promptForVisualStyleKey(key: string): string {
  if (key === '__custom__') return '';
  return VISUAL_STYLE_PRESETS.find((preset) => preset.key === key)?.prompt ?? DEFAULT_VISUAL_STYLE_PROMPT;
}

export function presetsInCategory(category: VisualStyleCategory): VisualStylePreset[] {
  return VISUAL_STYLE_PRESETS.filter((preset) => preset.category === category);
}
