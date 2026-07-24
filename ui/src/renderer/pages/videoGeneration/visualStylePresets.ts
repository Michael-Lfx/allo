/**
 * Curated visual styles for nomi-vimax cast & film prompts.
 * Prompts are English (pipeline / image models consume them as-is).
 * Stylized keys intentionally include needles from `wants_stylized_non_photoreal`.
 */

export interface VisualStylePreset {
  key: string;
  /** i18n key under `videoGeneration.workspace.source.stylePresets.*` */
  labelKey: string;
  /** Fallback label when i18n is missing. */
  defaultLabel: string;
  /** Prompt text stored on the session / sent to the backend. */
  prompt: string;
}

/** Matches backend `DEFAULT_VISUAL_STYLE` in nomi-vimax planning. */
export const DEFAULT_VISUAL_STYLE_PROMPT =
  'cinematic film look, believable designed characters, natural wardrobe and lighting, gently softened facial skin with clear readable features';

export const VISUAL_STYLE_PRESETS: readonly VisualStylePreset[] = [
  {
    key: 'cinematic',
    labelKey: 'videoGeneration.workspace.source.stylePresets.cinematic',
    defaultLabel: '电影写实',
    prompt: DEFAULT_VISUAL_STYLE_PROMPT,
  },
  {
    key: 'documentary',
    labelKey: 'videoGeneration.workspace.source.stylePresets.documentary',
    defaultLabel: '纪录片',
    prompt:
      'documentary cinema look, natural ambient light, handheld intimacy, gently softened faces with clear features, authentic wardrobe and locations',
  },
  {
    key: 'vintageFilm',
    labelKey: 'videoGeneration.workspace.source.stylePresets.vintageFilm',
    defaultLabel: '复古胶片',
    prompt:
      '35mm vintage film look, Kodak Portra color science, soft halation, fine grain, warm highlights, cinematic framing, gently softened facial skin with clear readable features',
  },
  {
    key: 'noir',
    labelKey: 'videoGeneration.workspace.source.stylePresets.noir',
    defaultLabel: '黑色电影',
    prompt:
      'classic film noir, high-contrast black and white cinematography, dramatic chiaroscuro lighting, deep shadows, anamorphic bokeh, gently softened faces with clear features',
  },
  {
    key: 'cyberpunk',
    labelKey: 'videoGeneration.workspace.source.stylePresets.cyberpunk',
    defaultLabel: '赛博朋克',
    prompt:
      'cyberpunk neo-noir night city, neon rim light, rain-slick streets, teal-and-magenta grade, volumetric haze, cinematic composition, gently softened faces with clear readable features',
  },
  {
    key: 'commercial',
    labelKey: 'videoGeneration.workspace.source.stylePresets.commercial',
    defaultLabel: '商业广告',
    prompt:
      'premium commercial advertising look, clean keyed lighting, crisp product-grade detail, shallow depth of field, polished color grade, gently softened facial skin with clear features',
  },
  {
    key: 'anime',
    labelKey: 'videoGeneration.workspace.source.stylePresets.anime',
    defaultLabel: '日式动画',
    prompt:
      'theatrical anime / animated-film character design, clear volume and soft painted shading, detailed hair strands and fabric folds, rich wardrobe materials, storybook colors — NOT flat paper-doll cel cutout, NOT photoreal live-action',
  },
  {
    key: 'ghibli',
    labelKey: 'videoGeneration.workspace.source.stylePresets.ghibli',
    defaultLabel: '吉卜力风',
    prompt:
      'Studio Ghibli inspired hand-drawn animation, soft watercolor backgrounds, warm natural light, gentle character acting, painterly sky and foliage, cinematic anime composition — NOT photoreal live-action',
  },
  {
    key: 'pixar3d',
    labelKey: 'videoGeneration.workspace.source.stylePresets.pixar3d',
    defaultLabel: '3D 动画电影',
    prompt:
      'Pixar / Disney style 3D animated film, appealing character design, subsurface skin, detailed cloth and hair, cinematic lighting and camera language, warm family-film color — NOT photoreal live-action',
  },
  {
    key: 'illustration',
    labelKey: 'videoGeneration.workspace.source.stylePresets.illustration',
    defaultLabel: '绘本插画',
    prompt:
      'painted illustration style, detailed brushwork, storybook atmosphere, cinematic composition (not anime), expressive designed characters',
  },
  {
    key: 'watercolor',
    labelKey: 'videoGeneration.workspace.source.stylePresets.watercolor',
    defaultLabel: '水彩手绘',
    prompt:
      'soft watercolor hand-drawn illustration, translucent washes, paper texture, gentle edges, storybook palette, cinematic framing — NOT photoreal live-action',
  },
  {
    key: 'comic',
    labelKey: 'videoGeneration.workspace.source.stylePresets.comic',
    defaultLabel: '漫画分镜',
    prompt:
      'graphic novel / comic book style, bold ink linework, dramatic panel lighting, rich flat-to-cel color, cinematic comic composition — NOT photoreal live-action',
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
