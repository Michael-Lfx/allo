/**
 * Curated visual styles for nomi-vimax cast & film prompts.
 *
 * Catalog shaped by mainstream video tools (Runway / Kling / Pika / Higgsfield /
 * Vidu / 即梦 / 剪映 / Midjourney) plus huobao-drama style seeds (3D 漫剧, 日漫赛璐璐)
 * and short-drama live-action looks (韩剧 / 港片 / 仙侠 / 都市言情). Live-action film
 * looks, genre moods, animation media, illustration craft, and crafted / social
 * aesthetics — one row is one persistent medium, not a camera move or transition.
 *
 * Prompts are English (pipeline / image models consume them as-is).
 * Stylized keys intentionally include needles from `wants_stylized_non_photoreal`.
 * Labels and prompts describe craft (watercolor, 2D, brick stop-motion), not studio or director names.
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

/**
 * Pipeline fallback when a run is submitted with no look selected.
 * Matches backend `DEFAULT_VISUAL_STYLE` in nomi-vimax planning.
 * The home / workspace pickers must not auto-select this — empty style means unset.
 */
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
  {
    key: 'kDrama',
    category: 'liveAction',
    labelKey: 'videoGeneration.workspace.source.stylePresets.kDrama',
    defaultLabel: '韩剧质感',
    prompt:
      'Korean drama live-action cinema, luminous beauty lighting, clean Seoul interiors, soft contrast, polished contemporary wardrobe, intimate two-shot framing, clean healthy facial skin with clear readable features',
  },
  {
    key: 'hkCinema',
    category: 'liveAction',
    labelKey: 'videoGeneration.workspace.source.stylePresets.hkCinema',
    defaultLabel: '港片风',
    prompt:
      'Hong Kong genre cinema, neon-soaked night streets, kinetic practical lighting, slightly warm tungsten mixed with cool fluorescents, lived-in urban texture, clean healthy faces with clear features',
  },
  {
    key: 'idolMv',
    category: 'liveAction',
    labelKey: 'videoGeneration.workspace.source.stylePresets.idolMv',
    defaultLabel: '偶像 MV',
    prompt:
      'idol music-video live-action look, high-key beauty lighting, glossy costume and set design, rhythmic camera, saturated but clean color grade, clean healthy facial skin with clear readable features',
  },
  {
    key: 'a24Muted',
    category: 'liveAction',
    labelKey: 'videoGeneration.workspace.source.stylePresets.a24Muted',
    defaultLabel: '文艺冷调',
    prompt:
      'prestige indie live-action cinema, muted earth palette, naturalistic available light, quiet negative space, restrained contrast, observational framing, clean healthy faces with clear features',
  },
  {
    key: 'imax70mm',
    category: 'liveAction',
    labelKey: 'videoGeneration.workspace.source.stylePresets.imax70mm',
    defaultLabel: '巨幕 IMAX',
    prompt:
      'large-format 70mm IMAX live-action cinema, immense vertical scale, ultra-fine grain, deep focus landscapes, immersive wide compositions, clean healthy facial skin with clear readable features',
  },
  {
    key: 'sixteenMm',
    category: 'liveAction',
    labelKey: 'videoGeneration.workspace.source.stylePresets.sixteenMm',
    defaultLabel: '16mm 胶片',
    prompt:
      '16mm film live-action look, visible organic grain, slightly warm highlights, gentle halation, intimate documentary framing, tactile analog texture, clean healthy faces with clear features',
  },
  {
    key: 'foundFootage',
    category: 'liveAction',
    labelKey: 'videoGeneration.workspace.source.stylePresets.foundFootage',
    defaultLabel: '伪纪录',
    prompt:
      'found-footage live-action cinema, consumer-camera framing, available light, diegetic shake, raw observational grade, diegetic date-stamp atmosphere without overlay text, clean healthy faces with clear features',
  },
  {
    key: 'urbanRomance',
    category: 'liveAction',
    labelKey: 'videoGeneration.workspace.source.stylePresets.urbanRomance',
    defaultLabel: '都市言情',
    prompt:
      'Chinese urban romance live-action drama, contemporary city interiors, soft beauty key light, glossy wardrobe, warm-cool mixed practicals, intimate two-shot blocking, clean healthy facial skin with clear readable features',
  },
  {
    key: 'republicanEra',
    category: 'liveAction',
    labelKey: 'videoGeneration.workspace.source.stylePresets.republicanEra',
    defaultLabel: '民国年代',
    prompt:
      'Republican-era Chinese period live-action cinema, cheongsam and tailored coats, amber tungsten practicals, rain-slick cobblestones, restrained 1930s production design, clean healthy faces with clear features',
  },
  {
    key: 'campusYouth',
    category: 'liveAction',
    labelKey: 'videoGeneration.workspace.source.stylePresets.campusYouth',
    defaultLabel: '校园青春',
    prompt:
      'campus youth live-action drama, sunlit school courtyards, school-uniform wardrobe, high-key daylight, light bloom, coming-of-age color, clean healthy facial skin with clear readable features',
  },
  {
    key: 'pastelTableau',
    category: 'liveAction',
    labelKey: 'videoGeneration.workspace.source.stylePresets.pastelTableau',
    defaultLabel: '糖果对称',
    prompt:
      'centered-symmetry tableau live-action cinema, pastel production design, front-on locked camera, dollhouse color blocking, dry deadpan timing, meticulously dressed sets, clean healthy faces with clear features',
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
  {
    key: 'xianxia',
    category: 'genreMood',
    labelKey: 'videoGeneration.workspace.source.stylePresets.xianxia',
    defaultLabel: '仙侠古装',
    prompt:
      'Chinese xianxia live-action cinema, flowing hanfu silk, misty mountain palaces, jade and gold accents, volumetric dawn light, period-accurate wardrobe, clean healthy facial skin with clear readable features',
  },
  {
    key: 'steampunk',
    category: 'genreMood',
    labelKey: 'videoGeneration.workspace.source.stylePresets.steampunk',
    defaultLabel: '蒸汽朋克',
    prompt:
      'steampunk live-action cinema, brass gears and riveted metal, warm gaslamp practicals, sepia-amber grade, Victorian-industrial wardrobe, cinematic framing, clean healthy faces with clear features',
  },
  {
    key: 'vaporwave',
    category: 'genreMood',
    labelKey: 'videoGeneration.workspace.source.stylePresets.vaporwave',
    defaultLabel: '蒸汽波',
    prompt:
      'vaporwave live-action look, pastel magenta-and-cyan grade, chrome and marble sets, retro-futurist 80s lighting, dreamy bloom, clean healthy facial skin with clear readable features',
  },
  {
    key: 'wuxia',
    category: 'genreMood',
    labelKey: 'videoGeneration.workspace.source.stylePresets.wuxia',
    defaultLabel: '武侠江湖',
    prompt:
      'Chinese wuxia live-action cinema, bamboo forests and inn courtyards, martial-arts silk wardrobe, dust motes in shaft light, earthy period grade, kinetic but photoreal fighting, clean healthy faces with clear features',
  },
  {
    key: 'darkFantasy',
    category: 'genreMood',
    labelKey: 'videoGeneration.workspace.source.stylePresets.darkFantasy',
    defaultLabel: '暗黑奇幻',
    prompt:
      'dark-fantasy live-action cinema, torchlit stone halls, wet iron and worn leather, low-key chiaroscuro, desaturated moss-and-ember palette, mythic production design, clean healthy faces with clear readable features',
  },
  {
    key: 'postApocalyptic',
    category: 'genreMood',
    labelKey: 'videoGeneration.workspace.source.stylePresets.postApocalyptic',
    defaultLabel: '废土末世',
    prompt:
      'post-apocalyptic live-action cinema, bleached daylight, rust and dust textures, scavenged wardrobe, long empty landscapes, muted ochre grade, clean healthy faces with clear features',
  },
  {
    key: 'thrillerTeal',
    category: 'genreMood',
    labelKey: 'videoGeneration.workspace.source.stylePresets.thrillerTeal',
    defaultLabel: '悬疑冷青',
    prompt:
      'contemporary thriller live-action cinema, teal-and-amber night grade, hard practicals, rain-wet streets, tense negative fill, precise blocking, clean healthy facial skin with clear readable features',
  },
  {
    key: 'warEpic',
    category: 'genreMood',
    labelKey: 'videoGeneration.workspace.source.stylePresets.warEpic',
    defaultLabel: '战争史诗',
    prompt:
      'war-epic live-action cinema, muddy earth and smoke, overcast natural light, large-scale troop staging, desaturated khaki grade, tactile uniforms and gear, clean healthy faces with clear features',
  },

  // —— Animation / 3D (huobao 3D 漫剧, Pika, Higgsfield, Vidu; genre craft, not studio names) ——
  {
    key: 'manhua3d',
    category: 'animation',
    labelKey: 'videoGeneration.workspace.source.stylePresets.manhua3d',
    defaultLabel: '3D 漫剧',
    prompt:
      '3D CG animation style, game-engine quality render, semi-realistic stylized characters, refined facial features, detailed materials and textures, cinematic lighting, high detail — NOT photoreal live-action, NOT flat 2D cel',
  },
  {
    key: 'toonShader3d',
    category: 'animation',
    labelKey: 'videoGeneration.workspace.source.stylePresets.toonShader3d',
    defaultLabel: '三渲二',
    prompt:
      'toon-shaded 3D animation, 2D anime lighting on 3D models, hard cel shadow bands, clean contour lines over sculpted forms — NOT photoreal live-action, NOT flat paper cutout',
  },
  {
    key: 'anime',
    category: 'animation',
    labelKey: 'videoGeneration.workspace.source.stylePresets.anime',
    defaultLabel: '日式动画',
    prompt:
      'theatrical anime / animated-film character design, clear volume and soft painted shading, detailed hair strands and fabric folds, rich wardrobe materials, storybook colors — NOT flat paper-doll cel cutout, NOT photoreal live-action',
  },
  {
    key: 'animeCel',
    category: 'animation',
    labelKey: 'videoGeneration.workspace.source.stylePresets.animeCel',
    defaultLabel: '日漫赛璐璐',
    prompt:
      'Japanese anime style, cel shading, clean crisp line art, vivid saturated colors, expressive character designs, detailed painted backgrounds — NOT photoreal live-action',
  },
  {
    key: 'ghibli',
    category: 'animation',
    labelKey: 'videoGeneration.workspace.source.stylePresets.ghibli',
    defaultLabel: '水彩手绘动画',
    prompt:
      'hand-drawn animation, soft watercolor backgrounds, warm natural light, gentle character acting, painterly sky and foliage, cinematic anime composition — NOT photoreal live-action',
  },
  {
    key: 'shinkai',
    category: 'animation',
    labelKey: 'videoGeneration.workspace.source.stylePresets.shinkai',
    defaultLabel: '晴空细绘动画',
    prompt:
      'cinematic anime, hyper-detailed painted skies, luminous god-rays, crisp urban and landscape backgrounds, emotional close-ups, cinematic anime composition — NOT photoreal live-action',
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
      'feature 3D CG animation film, appealing character design, subsurface skin, detailed cloth and hair, cinematic lighting and camera language, warm family-film color — NOT photoreal live-action',
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
  {
    key: 'americanCartoon',
    category: 'animation',
    labelKey: 'videoGeneration.workspace.source.stylePresets.americanCartoon',
    defaultLabel: '美式卡通',
    prompt:
      'American cartoon animation, bold graphic shapes, squash-and-stretch appeal, flat vibrant color, clean outlines, Saturday-morning energy — NOT photoreal live-action, NOT Japanese anime',
  },
  {
    key: 'lowPoly',
    category: 'animation',
    labelKey: 'videoGeneration.workspace.source.stylePresets.lowPoly',
    defaultLabel: '低多边形',
    prompt:
      'low-poly 3D animation, faceted geometric forms, limited color palette, clean ambient occlusion, stylized game-art lighting — NOT photoreal live-action, NOT smooth subdivision-surface CGI',
  },
  {
    key: 'unrealCinematic',
    category: 'animation',
    labelKey: 'videoGeneration.workspace.source.stylePresets.unrealCinematic',
    defaultLabel: 'UE 电影级',
    prompt:
      'Unreal Engine cinematic 3D animation, real-time path-traced lighting, stylized game characters with film cameras, high-fidelity materials — NOT photoreal live-action photography, NOT flat 2D cel',
  },
  {
    key: 'disney2d',
    category: 'animation',
    labelKey: 'videoGeneration.workspace.source.stylePresets.disney2d',
    defaultLabel: '古典二维手绘',
    prompt:
      'classical 2D animation, clean construction, squash-and-stretch appeal, painted backgrounds, theatrical staging, hand-drawn character acting — NOT photoreal live-action, NOT Japanese anime',
  },
  {
    key: 'chibi',
    category: 'animation',
    labelKey: 'videoGeneration.workspace.source.stylePresets.chibi',
    defaultLabel: 'Q 版萌系',
    prompt:
      'chibi animation, super-deformed cute proportions, oversized heads, simple rounded limbs, pastel candy color, playful staging — NOT photoreal live-action, NOT realistic anatomy',
  },
  {
    key: 'mechaAnime',
    category: 'animation',
    labelKey: 'videoGeneration.workspace.source.stylePresets.mechaAnime',
    defaultLabel: '机甲动画',
    prompt:
      'mecha anime, mechanical hard-surface robots, panel-line cel shading, dramatic sky backgrounds, theatrical anime composition — NOT photoreal live-action, NOT soft toy 3D',
  },
  {
    key: 'legoBrickfilm',
    category: 'animation',
    labelKey: 'videoGeneration.workspace.source.stylePresets.legoBrickfilm',
    defaultLabel: '积木定格',
    prompt:
      'plastic brickfilm stop-motion animation, visible plastic studs and clutch, miniature brick-built sets, clicky frame-step motion, studio product light — NOT photoreal live-action, NOT smooth CGI',
  },
  {
    key: 'feltWool',
    category: 'animation',
    labelKey: 'videoGeneration.workspace.source.stylePresets.feltWool',
    defaultLabel: '毛毡偶戏',
    prompt:
      'needle-felted wool stop-motion animation, fuzzy felted characters, visible fiber texture, handmade miniature sets, warm tactile lighting — NOT photoreal live-action, NOT smooth 3D plastic',
  },
  {
    key: 'crayonKids',
    category: 'animation',
    labelKey: 'videoGeneration.workspace.source.stylePresets.crayonKids',
    defaultLabel: '蜡笔童书',
    prompt:
      'children crayon illustration animation, waxy crayon strokes, construction-paper grain, naive charming drawing, storybook staging — NOT photoreal live-action, NOT slick digital paint',
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
  {
    key: 'ukiyoE',
    category: 'illustration',
    labelKey: 'videoGeneration.workspace.source.stylePresets.ukiyoE',
    defaultLabel: '浮世绘',
    prompt:
      'ukiyo-e woodblock illustration, flat layered color, visible grain and baren texture, bold contour lines, Edo print composition, hand-printed atmosphere — NOT photoreal live-action',
  },
  {
    key: 'paperCut',
    category: 'illustration',
    labelKey: 'videoGeneration.workspace.source.stylePresets.paperCut',
    defaultLabel: '剪纸定格',
    prompt:
      'paper-cut stop-motion animation, layered colored paper silhouettes, visible cut edges and slight shadow gaps, folk-craft lighting, tactile miniature sets — NOT photoreal live-action',
  },
  {
    key: 'charcoal',
    category: 'illustration',
    labelKey: 'videoGeneration.workspace.source.stylePresets.charcoal',
    defaultLabel: '炭笔素描',
    prompt:
      'charcoal drawing illustration, dusty compressed-charcoal strokes, paper tooth, dramatic tonal modeling, sketchbook cinematic framing — NOT photoreal live-action photography',
  },
  {
    key: 'lineArt',
    category: 'illustration',
    labelKey: 'videoGeneration.workspace.source.stylePresets.lineArt',
    defaultLabel: '线稿平涂',
    prompt:
      'clean line-art illustration, uniform ink contours, flat limited color fills, graphic poster composition, crisp digital inking — NOT photoreal live-action, NOT painterly oil',
  },
  {
    key: 'dunhuang',
    category: 'illustration',
    labelKey: 'videoGeneration.workspace.source.stylePresets.dunhuang',
    defaultLabel: '敦煌壁画',
    prompt:
      'Dunhuang mural painting illustration, mineral pigment fresco, flying apsaras linework, aged plaster texture, Buddhist cave-art palette, painted not photographed — NOT photoreal live-action',
  },
  {
    key: 'stainedGlass',
    category: 'illustration',
    labelKey: 'videoGeneration.workspace.source.stylePresets.stainedGlass',
    defaultLabel: '彩绘玻璃',
    prompt:
      'stained-glass illustration, leaded black cames, jewel-tone translucent panes, cathedral backlight, mosaic narrative panels — NOT photoreal live-action photography',
  },
  {
    key: 'artNouveau',
    category: 'illustration',
    labelKey: 'videoGeneration.workspace.source.stylePresets.artNouveau',
    defaultLabel: '新艺术',
    prompt:
      'Art Nouveau illustration, whiplash botanical line, decorative posters, gold and muted jewel tones, Mucha-like ornamental frames without lettering — NOT photoreal live-action',
  },
  {
    key: 'popArt',
    category: 'illustration',
    labelKey: 'videoGeneration.workspace.source.stylePresets.popArt',
    defaultLabel: '波普印刷',
    prompt:
      'Pop art print illustration, Ben-Day dots, hard complementary flats, comic-advertising graphics, silkscreen texture — NOT photoreal live-action photography',
  },
  {
    key: 'shadowPuppet',
    category: 'illustration',
    labelKey: 'videoGeneration.workspace.source.stylePresets.shadowPuppet',
    defaultLabel: '皮影戏',
    prompt:
      'Chinese shadow puppet animation, translucent leather silhouettes, articulated joints, backlit parchment screen, warm oil-lamp glow — NOT photoreal live-action',
  },
  {
    key: 'pastelGouache',
    category: 'illustration',
    labelKey: 'videoGeneration.workspace.source.stylePresets.pastelGouache',
    defaultLabel: '水粉平涂',
    prompt:
      'gouache illustration, opaque matte pigment, soft-edged color blocking, Morandi muted palette, hand-painted poster atmosphere — NOT photoreal live-action',
  },
  {
    key: 'shoujoManga',
    category: 'illustration',
    labelKey: 'videoGeneration.workspace.source.stylePresets.shoujoManga',
    defaultLabel: '少女漫画',
    prompt:
      'shoujo manga illustration, glitter eyes, floral screentones, delicate linework, romantic panel lighting, shojo comic composition — NOT photoreal live-action',
  },
  {
    key: 'lianhuanhua',
    category: 'illustration',
    labelKey: 'videoGeneration.workspace.source.stylePresets.lianhuanhua',
    defaultLabel: '连环画',
    prompt:
      'Chinese lianhuanhua illustration, compact inked panels, economical line, period storybook staging, printed-paper graphic novel look — NOT photoreal live-action',
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
  {
    key: 'voxel',
    category: 'crafted',
    labelKey: 'videoGeneration.workspace.source.stylePresets.voxel',
    defaultLabel: '体素方块',
    prompt:
      'voxel 3D animation, cubic blocky forms, limited Minecraft-like palette, chunky ambient occlusion, toy-scale cubic worlds — NOT photoreal live-action, NOT smooth subdivision surfaces',
  },
  {
    key: 'origami',
    category: 'crafted',
    labelKey: 'videoGeneration.workspace.source.stylePresets.origami',
    defaultLabel: '折纸定格',
    prompt:
      'origami stop-motion animation, folded paper sculpture characters, visible crease geometry, hard paper edges, clean tabletop lighting — NOT photoreal live-action, NOT cut-paper silhouettes',
  },
  {
    key: 'glitchArt',
    category: 'crafted',
    labelKey: 'videoGeneration.workspace.source.stylePresets.glitchArt',
    defaultLabel: '故障艺术',
    prompt:
      'glitch art video, datamosh pixel smear, RGB channel split, codec-block artifacts, digital decay aesthetic — NOT photoreal clean cinema, NOT analog film grain',
  },
  {
    key: 'polaroid',
    category: 'crafted',
    labelKey: 'videoGeneration.workspace.source.stylePresets.polaroid',
    defaultLabel: '拍立得',
    prompt:
      'instant Polaroid live-action look, square faded frame, chemical color shift, soft flash, nostalgic snapshot staging, clean healthy faces with clear features',
  },
  {
    key: 'super8',
    category: 'crafted',
    labelKey: 'videoGeneration.workspace.source.stylePresets.super8',
    defaultLabel: 'Super 8',
    prompt:
      'Super 8 home-movie live-action look, warm gate weave, light leaks, saturated vintage stock, handheld family framing, clean healthy faces with clear features',
  },
] as const;

export function findVisualStylePreset(style: string | undefined | null): VisualStylePreset | undefined {
  const trimmed = (style ?? '').trim();
  if (!trimmed) return undefined;
  return VISUAL_STYLE_PRESETS.find((preset) => preset.prompt === trimmed);
}

/** Select value for a stored style string. Empty / unset → `''` (no look). */
export function visualStyleSelectValue(style: string | undefined | null): string {
  const trimmed = (style ?? '').trim();
  if (!trimmed) return '';
  return findVisualStylePreset(trimmed)?.key ?? '__custom__';
}

export function promptForVisualStyleKey(key: string): string {
  const trimmed = key.trim();
  if (!trimmed || trimmed === '__custom__') return '';
  return VISUAL_STYLE_PRESETS.find((preset) => preset.key === trimmed)?.prompt ?? DEFAULT_VISUAL_STYLE_PROMPT;
}

export function presetsInCategory(category: VisualStyleCategory): VisualStylePreset[] {
  return VISUAL_STYLE_PRESETS.filter((preset) => preset.category === category);
}

/** Keys migrated from huobao-drama `stylePresetSeeds` (3d / anime). Watercolor and comic already existed. */
export const HUOBAO_LOOK_KEYS = ['manhua3d', 'animeCel'] as const;

/** True when the user picked a look (catalog or custom). Empty is a valid choice. */
export function hasSelectedVisualStyle(style: string | undefined | null): boolean {
  return (style ?? '').trim().length > 0;
}

/** Unset look — the picker shows 「画风」 and generation may still apply the cinematic fallback. */
export function isDefaultVisualStyle(style: string | undefined | null): boolean {
  return !hasSelectedVisualStyle(style);
}
