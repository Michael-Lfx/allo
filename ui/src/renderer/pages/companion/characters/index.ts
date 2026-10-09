

import Puff from './Puff';
import Mochi from './Mochi';
import Ink from './Ink';
import Bolt from './Bolt';
import { customDeskSpec } from './customDesk';
import type { CharacterDeskSpec, CharacterMeta, CustomFigureMeta } from './types';

export type {
  CharacterDeskSpec,
  CharacterMeta,
  CharacterProps,
  CustomFigureMeta,
  CompanionActivity,
  CompanionMood,
  CompanionMotion,
} from './types';

/**
 * The character roster. Order = display order in the picker.
 * `palette` feeds the little swatch chip on each picker card.
 */
export const CHARACTERS: CharacterMeta[] = [
  { id: 'puff', nameKey: 'puff', palette: ['#f4f1ea', '#1a1a1e'], Component: Puff },
  { id: 'mochi', nameKey: 'mochi', palette: ['#fff6f0', '#ffb7c9'], Component: Mochi },
  { id: 'ink', nameKey: 'ink', palette: ['#2b2b33', '#e8b04b'], Component: Ink },
  { id: 'bolt', nameKey: 'bolt', palette: ['#bfeee0', '#37e0ff'], Component: Bolt },
];

/** New companions pick Puff. Unknown persisted ids still fall back to Mochi. */
export const DEFAULT_CHARACTER_ID = 'puff';

export const getCharacter = (id?: string | null): CharacterMeta =>
  CHARACTERS.find((c) => c.id === id) ?? CHARACTERS.find((c) => c.id === 'mochi') ?? CHARACTERS[0];

/**
 * The classic chibi window every character used before per-character desks.
 * Must match the creation-time `inner_size` in apps/desktop/src/main.rs —
 * applyDeskSize self-heals a mismatch with a visible startup resize.
 * Height = figure (150) + chrome (~64: hover chat bar reserve + hop headroom);
 * the bubble's room is grown on demand (enterChatSize), not reserved here, so the
 * idle window hugs the figure instead of parking a tall transparent strip on the
 * desktop.
 */
export const DEFAULT_DESK: CharacterDeskSpec = { windowWidth: 240, windowHeight: 214, figureHeight: 150 };

export const getDeskSpec = (id?: string | null): CharacterDeskSpec => getCharacter(id).desk ?? DEFAULT_DESK;

export const CUSTOM_CHARACTER_ID = 'custom';

/** Desk spec resolution that understands DIY custom figures. */
export const getDeskSpecFor = (id?: string | null, meta?: CustomFigureMeta | null): CharacterDeskSpec =>
  id === CUSTOM_CHARACTER_ID && meta ? customDeskSpec(meta) : getDeskSpec(id);
