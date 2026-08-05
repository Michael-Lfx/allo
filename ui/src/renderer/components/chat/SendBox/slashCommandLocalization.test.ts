/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

import enConversation from '../../../services/i18n/locales/en-US/conversation.json';
import zhConversation from '../../../services/i18n/locales/zh-CN/conversation.json';

const source = readFileSync(new URL('./index.tsx', import.meta.url), 'utf8');

describe('SendBox slash command localization', () => {
  test('keeps /compact command description in locale files and renders it through i18n', () => {
    expect(zhConversation.slashCommands?.compact?.description).toBe('压缩会话上下文');
    expect(enConversation.slashCommands?.compact?.description).toBe('Compress conversation context');

    expect(source.includes("command.name === 'compact'")).toBe(true);
    expect(source.includes('conversation.slashCommands.compact.description')).toBe(true);
    expect(source.includes('description: getSlashCommandDescription(command, t)')).toBe(true);
  });

  test('keeps backend-declared host commands in the system group without name guessing', () => {
    expect(source.includes("command.source === 'builtin' ? ('system' as const) : ('agent' as const)")).toBe(true);
    expect(source.includes("id: `${command.source === 'builtin' ? 'system' : 'agent'}:${command.source}:${command.name}`")).toBe(true);
    expect(source.includes('const goalInvocation = parseGoalSlashCommand')).toBe(true);
    expect(source.includes('groupSlashLauncherItems(launcherItems)')).toBe(true);
    expect(source.includes('items: orderedLauncherItems')).toBe(true);
  });

  test('aligns an open command menu with the composer and keeps its border neutral', () => {
    expect(source.includes('const isComposerMenuOpen = isCommandMenuOpen || isAddMenuOpen || isAtFileMenuOpen')).toBe(true);
    expect(source.includes("left-0 right-0 bottom-[calc(100%+10px)] z-70")).toBe(true);
    expect(source.includes('borderColor: isComposerMenuOpen')).toBe(true);
    expect(source.includes("boxShadow: 'none'")).toBe(true);
  });
});
