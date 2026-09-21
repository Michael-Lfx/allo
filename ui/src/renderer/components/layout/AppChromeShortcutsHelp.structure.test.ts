import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

const helpSource = readFileSync(new URL('./AppChromeShortcutsHelp.tsx', import.meta.url), 'utf8');
const hostSource = readFileSync(new URL('./AppChromeShortcutsHost.tsx', import.meta.url), 'utf8');
const layoutSource = readFileSync(new URL('./Layout.tsx', import.meta.url), 'utf8');
const footerSource = readFileSync(new URL('./Sider/SiderFooter.tsx', import.meta.url), 'utf8');
const searchEntrySource = readFileSync(new URL('./Sider/SiderNav/SiderSearchEntry.tsx', import.meta.url), 'utf8');
const titlebarSource = readFileSync(new URL('./Titlebar/index.tsx', import.meta.url), 'utf8');
const systemSource = readFileSync(
  new URL('../settings/SettingsModal/contents/SystemModalContent/index.tsx', import.meta.url),
  'utf8'
);

describe('app chrome shortcut surfaces', () => {
  test('lists the planned chrome actions and never mentions logout', () => {
    expect(helpSource.includes("listAppChromeShortcutsForSheet")).toBe(true);
    expect(helpSource.includes("'navigation'")).toBe(true);
    expect(helpSource.includes("'conversation'")).toBe(true);
    expect(helpSource.includes("'help'")).toBe(true);
    expect(helpSource.includes('common.shortcuts.newConversation')).toBe(false);
    expect(helpSource.includes('item.titleKey')).toBe(true);
    expect(helpSource.includes('logout')).toBe(false);
    expect(helpSource.includes('data-testid=\'app-chrome-shortcuts-help\'')).toBe(true);
    expect(helpSource.includes('grid size-26px place-items-center')).toBe(true);
    expect(helpSource.includes('[&_.i-icon]:block')).toBe(true);
    expect(helpSource.includes("className='block leading-none'")).toBe(true);
  });

  test('hosts the listener inside layout and opens from settings', () => {
    expect(layoutSource.includes('AppChromeShortcutsHost')).toBe(true);
    expect(hostSource.includes('useAppChromeShortcuts')).toBe(true);
    expect(hostSource.includes("emitter.emit('composer.focus')")).toBe(true);
    expect(hostSource.includes("navigate('/guid', { state: { resetPreset: true } })")).toBe(true);
    expect(systemSource.includes("emitter.emit('app.shortcuts.open')")).toBe(true);
    expect(systemSource.includes("t('settings.viewShortcuts')")).toBe(true);
  });

  test('puts shortcuts on sidebar, settings, collapsed search, and new-chat chrome', () => {
    expect(titlebarSource.includes("appChromeShortcutTooltip(siderTooltipBase, 'sidebar')")).toBe(true);
    expect(titlebarSource.includes("appChromeShortcutTooltip(newConversationTooltipBase, 'newConversation')")).toBe(
      true
    );
    expect(footerSource.includes("appChromeShortcutTooltip(settingsTooltipBase, 'settings')")).toBe(true);
    expect(searchEntrySource.includes("appChromeShortcutTooltip(t('conversation.historySearch.tooltip'), 'search')")).toBe(
      true
    );
  });
});
