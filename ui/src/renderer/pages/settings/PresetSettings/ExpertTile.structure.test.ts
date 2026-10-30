import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const tileSource = readFileSync(new URL('./ExpertTile.tsx', import.meta.url), 'utf8');

describe('ExpertTile summon action', () => {
  test('shows the summon button only while the card is hovered or focused', () => {
    expect(tileSource.includes('pointer-events-none opacity-0')).toBe(true);
    expect(tileSource.includes('group-hover:pointer-events-auto group-hover:opacity-100')).toBe(true);
    expect(tileSource.includes('group-focus-within:pointer-events-auto group-focus-within:opacity-100')).toBe(true);
    expect(tileSource.includes('actionPending || actionDone')).toBe(true);
  });

  test('renders created-expert avatars as emoji or resolved images', () => {
    const catalogSource = readFileSync(new URL('./MyExpertCatalog.tsx', import.meta.url), 'utf8');

    expect(catalogSource.includes('avatarImageSrc={resolveAvatarImageSrc(preset.avatar, avatarImageMap)}')).toBe(true);
    expect(catalogSource.includes('avatarEmoji={')).toBe(true);
    expect(tileSource.includes('<span className={emojiClass}>{emoji}</span>')).toBe(true);
  });
});
