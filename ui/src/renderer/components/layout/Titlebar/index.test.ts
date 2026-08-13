import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'fs';

const titlebarSource = readFileSync(new URL('./index.tsx', import.meta.url), 'utf8');
const titlebarStyles = readFileSync(new URL('./titlebar.css', import.meta.url), 'utf8');

describe('Titlebar instant icon tooltips', () => {
  test('uses delayed hover tooltips for icon-only titlebar actions', () => {
    expect(titlebarSource.includes('InstantHoverTooltip')).toBe(true);
    expect(titlebarSource.includes('hoverDelayMs={400}')).toBe(true);
    expect(titlebarSource.includes("position={position ?? 'bottom'}")).toBe(true);
    expect(titlebarSource.includes("className='app-titlebar__tooltip-anchor'")).toBe(true);
  });

  test('does not use native title fallbacks for titlebar icon buttons', () => {
    expect(titlebarSource.includes('title={historyBackTooltip}')).toBe(false);
    expect(titlebarSource.includes('title={historyForwardTooltip}')).toBe(false);
    expect(titlebarSource.includes("title={t('terminal.newConversation')}")).toBe(false);
    expect(titlebarSource.includes("title={t('terminal.newTerminal')}")).toBe(false);
    expect(titlebarSource.includes('title={sessionToggleTooltip}')).toBe(false);
    expect(titlebarSource.includes('TitlebarLanguageMenu')).toBe(false);
  });

  test('shows the workspace titlebar toggle on mobile only', () => {
    expect(titlebarSource.includes('const showWorkspaceButton = workspaceAvailable && Boolean(layout?.isMobile);')).toBe(
      true
    );
  });

  test('uses a stable three-column desktop layout and limits new-conversation to chats and Settings', () => {
    expect(titlebarSource.includes("'app-titlebar--wide': !layout?.isMobile")).toBe(true);
    expect(titlebarSource.includes("data-titlebar-group='navigation'")).toBe(true);
    expect(titlebarSource.includes("data-titlebar-group='new-conversation'")).toBe(true);
    expect(titlebarSource.includes('const showNewConversationAction =')).toBe(true);
    expect(
      titlebarSource.includes("!layout?.isMobile && (isSettingsRoute || activeWorkspaceTarget?.kind === 'conversation')")
    ).toBe(true);
    expect(titlebarSource.includes('{showNewConversationAction && (')).toBe(true);
    expect(titlebarSource.includes('<NewConversationIcon')).toBe(true);
    expect(titlebarSource.includes('isSettingsRoute ? (')).toBe(true);
    expect(titlebarSource.includes('children: isSettingsRoute ? (')).toBe(true);
    expect(titlebarSource.includes('<HomeIcon')).toBe(true);
    expect(titlebarSource.includes("<path d='M24 15v14M17 22h14' />")).toBe(true);
    expect(titlebarSource.includes("children: <Plus theme='outline'")).toBe(false);
    expect(titlebarSource.includes("navigate('/guid', { state: { resetPreset: true } })")).toBe(true);
    expect(titlebarSource.includes('useTitlebarContextTitle(location.pathname)')).toBe(true);
    expect(titlebarStyles.includes('grid-template-columns: minmax(0, 1fr) minmax(0, 360px) minmax(0, 1fr)')).toBe(true);
    expect(titlebarStyles.includes('@media (max-width: 720px)')).toBe(true);
  });

  test('keeps the titlebar drag plane broad while marking interactive islands as no-drag', () => {
    expect(titlebarSource.includes('data-tauri-drag-region')).toBe(true);
    expect(titlebarSource.includes('data-tauri-no-drag')).toBe(true);
    expect(titlebarStyles).not.toMatch(/^\.app-titlebar__menu\s*\{\s*-webkit-app-region:\s*no-drag;/m);
    expect(titlebarStyles).not.toMatch(/^\.app-titlebar__toolbar\s*\{\s*[^}]*-webkit-app-region:\s*no-drag;/m);
    expect(titlebarStyles).toMatch(
      /\.app-titlebar__tooltip-anchor\s*\{[\s\S]*?-webkit-app-region:\s*no-drag;/,
    );
    expect(titlebarStyles).toMatch(
      /\.app-titlebar__actions-slot\s*\{[\s\S]*?-webkit-app-region:\s*no-drag;/,
    );
    expect(titlebarStyles).toMatch(
      /\.app-titlebar--mac\s+\.app-titlebar__menu,[\s\S]*?\.app-titlebar--mac\s+\.app-titlebar__toolbar\s*\{[\s\S]*?-webkit-app-region:\s*no-drag;/,
    );
  });

  test('only toggles maximize for a real drag-region target outside no-drag controls', () => {
    expect(titlebarSource.includes("target?.closest('[data-tauri-drag-region]')")).toBe(true);
    expect(titlebarSource.includes("target.closest('[data-tauri-no-drag]')")).toBe(true);
    expect(titlebarSource.includes('target.hasAttribute(\'data-tauri-drag-region\')')).toBe(false);
  });
});
