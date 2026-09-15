import { describe, expect, test } from 'bun:test';
import {
  HOME_IMAGE_MENTION_MENU_GAP_PX,
  placeHomeImageMentionMenu,
} from './mentionCaret';

describe('home image mention menu placement', () => {
  test('sits 8px below the @ glyph', () => {
    expect(
      placeHomeImageMentionMenu(
        { left: 120, top: 40, bottom: 64 },
        { width: 1280, height: 800 },
        { width: 256, estimatedHeight: 56 },
      ),
    ).toEqual({
      left: 120,
      top: 64 + HOME_IMAGE_MENTION_MENU_GAP_PX,
      width: 256,
    });
  });

  test('flips above the glyph when the viewport cannot fit below', () => {
    expect(
      placeHomeImageMentionMenu(
        { left: 40, top: 760, bottom: 784 },
        { width: 800, height: 800 },
        { width: 256, estimatedHeight: 120 },
      ),
    ).toEqual({
      left: 40,
      top: 760 - HOME_IMAGE_MENTION_MENU_GAP_PX - 120,
      width: 256,
    });
  });
});
