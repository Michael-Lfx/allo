import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const source = readFileSync(new URL('./SiderNewConversationEntry.tsx', import.meta.url), 'utf8');

describe('Sider new conversation shortcut chrome', () => {
  test('shows the desktop-only kbd badge with the shared search class', () => {
    expect(source.includes('APP_CHROME_SHORTCUT_BADGE_CLASS')).toBe(true);
    expect(source.includes("formatAppChromeShortcut('newConversation')")).toBe(true);
    expect(source.includes("isAppChromeShortcutAvailable('newConversation'")).toBe(true);
    expect(source.includes('desktop: isDesktopShell()')).toBe(true);
    expect(source.includes('mobile: isMobile')).toBe(true);
  });
});
