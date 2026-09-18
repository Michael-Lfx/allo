/**
 * Still thumbnails for Look chips. Files live in `ui/public/looks/{id}.webp`.
 * Canvas lookbook ids alias onto the matching vimax still — do not duplicate bytes.
 */
const COVER_FILE: Record<string, string> = {
  'urban-live-action': 'cinematic',
  'real-life-documentary': 'documentary',
  'campus-youth': 'campusYouth',
  'retro-hong-kong': 'vintageFilm',
  'period-live-action': 'xianxia',
  'suspense-noir': 'thrillerTeal',
  'black-white-noir': 'noir',
  'surreal-dream': 'dreamlike',
  'future-tech': 'sciFiClean',
  'cyberpunk-neon': 'cyberpunk',
  'chinese-2d': 'anime',
  'ink-narrative': 'inkWash',
  'comic-pop': 'comic',
  'three-d-cartoon': 'pixar3d',
  'fantasy-3d': 'manhua3d',
  'clay-stop-motion': 'claymation',
  'storybook-fantasy': 'illustration',
  'v2-wasteland--dark--live-action--realistic': 'postApocalyptic',
};

/** Exact lookbook 4-tuples → lookbook id (then aliased by COVER_FILE). */
const LOOKBOOK_AXES: Record<string, string> = {
  'urban--romantic--live-action--realistic': 'urban-live-action',
  'historical--epic--live-action--realistic': 'period-live-action',
  'suspense--dark--live-action--realistic': 'suspense-noir',
  'campus--romantic--live-action--realistic': 'campus-youth',
  'court--epic--live-action--realistic': 'court-pageant',
  'pastoral--healing--live-action--realistic': 'nature-healing',
  'urban--documentary--live-action--realistic': 'real-life-documentary',
  'republic--melancholic--live-action--realistic': 'retro-hong-kong',
  'urban--monochrome--live-action--realistic': 'black-white-noir',
  'urban--oneiric--live-action--realistic': 'surreal-dream',
  'science-fiction--epic--live-action--realistic': 'future-tech',
  'cyberpunk--dark--live-action--realistic': 'cyberpunk-neon',
  'space--epic--3d-anime--semi-real': 'space-opera',
  'xianxia--epic--2d-guoman--semi-real': 'chinese-2d',
  'xianxia--healing--ink--semi-real': 'ink-narrative',
  'urban--light-comedy--comic--stylized': 'comic-pop',
  'urban--light-comedy--3d-cartoon--stylized': 'three-d-cartoon',
  'xianxia--epic--3d-anime--semi-real': 'fantasy-3d',
  'urban--light-comedy--stop-motion--stylized': 'clay-stop-motion',
  'pastoral--healing--storybook--stylized': 'storybook-fantasy',
};

/** Recommended / custom mixes that are not a lookbook card. */
const COMBO_AXES: Record<string, string> = {
  'xianxia--dark--3d-anime--semi-real': 'darkFantasy',
  'xianxia--light-comedy--3d-cartoon--stylized': 'toonShader3d',
  'xianxia--epic--live-action--realistic': 'xianxia',
  'urban--light-comedy--live-action--realistic': 'commercial',
  'urban--melancholic--live-action--realistic': 'a24Muted',
  'urban--glamour--live-action--realistic': 'editorial',
  'historical--romantic--2d-guoman--semi-real': 'donghua',
  'pastoral--healing--3d-cartoon--stylized': 'ghibli',
  'wasteland--dark--live-action--realistic': 'postApocalyptic',
};

const MEDIUM_FILE: Record<string, string> = {
  'live-action': 'cinematic',
  '3d-anime': 'manhua3d',
  '3d-cartoon': 'pixar3d',
  '2d-guoman': 'anime',
  ink: 'inkWash',
  'stop-motion': 'claymation',
  comic: 'comic',
  storybook: 'illustration',
};

const LIVE_ACTION_TONE: Record<string, string> = {
  monochrome: 'noir',
  oneiric: 'dreamlike',
  documentary: 'documentary',
  glamour: 'editorial',
};

const LIVE_ACTION_WORLD: Record<string, string> = {
  cyberpunk: 'cyberpunk',
  wasteland: 'postApocalyptic',
  court: 'court-pageant',
  campus: 'campusYouth',
  pastoral: 'nature-healing',
  republic: 'vintageFilm',
  historical: 'xianxia',
  xianxia: 'xianxia',
  suspense: 'thrillerTeal',
  'science-fiction': 'sciFiClean',
  space: 'space-opera',
  urban: 'cinematic',
};

export type LookCoverAxes = {
  world: string;
  tone: string;
  medium: string;
  character: string;
};

function axesKey(axes: LookCoverAxes): string {
  return `${axes.world}--${axes.tone}--${axes.medium}--${axes.character}`;
}

function publicLooksBase(): string {
  const base = import.meta.env.BASE_URL || '/';
  return base.endsWith('/') ? base : `${base}/`;
}

export function lookCoverFileId(id: string): string {
  return COVER_FILE[id] ?? id;
}

export function lookCoverFileIdForAxes(axes: LookCoverAxes): string {
  const key = axesKey(axes);
  const lookbookId = LOOKBOOK_AXES[key];
  if (lookbookId) return lookCoverFileId(lookbookId);
  const comboId = COMBO_AXES[key];
  if (comboId) return comboId;
  if (axes.medium === 'live-action') {
    return LIVE_ACTION_TONE[axes.tone] ?? LIVE_ACTION_WORLD[axes.world] ?? MEDIUM_FILE['live-action'];
  }
  return MEDIUM_FILE[axes.medium] ?? 'cinematic';
}

export function lookCoverImage(id: string): string {
  return `${publicLooksBase()}looks/${lookCoverFileId(id)}.webp`;
}

export function lookCoverImageForAxes(axes: LookCoverAxes): string {
  return `${publicLooksBase()}looks/${lookCoverFileIdForAxes(axes)}.webp`;
}
