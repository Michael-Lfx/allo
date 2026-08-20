/**
 * @license
 * Copyright 2025-2026 NomiFun (nomifun.com)
 * SPDX-License-Identifier: Apache-2.0
 */

import { readFileSync } from 'node:fs';
import { describe, expect, test } from 'bun:test';

const readSource = (url: URL) => readFileSync(url, 'utf8');

describe('preview persistence entity isolation', () => {
  test('requires an explicit entity namespace and has no legacy fallback', () => {
    const source = readSource(new URL('./PreviewContext.tsx', import.meta.url));

    expect(source.includes('persistNamespace: string')).toBe(true);
    expect(source.includes('persistNamespace?: string')).toBe(false);
    expect(source.includes('DEFAULT_PERSIST_NAMESPACE')).toBe(false);
    expect(source.includes('legacyPreviewStateKey')).toBe(false);
    expect(source.includes("localStorage.getItem('nomifun_preview_state')")).toBe(false);
    expect(source.includes('getBrowserStorageGeneration()')).toBe(false);
    expect(source.includes('previewPersistenceNamespace')).toBe(false);
  });

  test('scopes conversation, terminal and transcript providers by stable entity id', () => {
    const chatLayout = readSource(new URL('../../components/ChatLayout/index.tsx', import.meta.url));
    const terminal = readSource(new URL('../../../terminal/TerminalSessionPage.tsx', import.meta.url));
    const transcript = readSource(new URL('../../execution/ReadOnlyConversationView.tsx', import.meta.url));

    expect(chatLayout.includes('persistNamespace={previewScope}')).toBe(true);
    expect(chatLayout.includes('key={previewScope}')).toBe(true);
    expect(chatLayout.includes("props.conversation_id ?? 'pending'")).toBe(false);
    expect(chatLayout.includes('conversation-pending:${uuid()}')).toBe(true);
    expect(chatLayout.includes("browserStorageKey('workspace-preview', 'conversation', props.conversation_id)")).toBe(true);
    expect(terminal.includes("browserStorageKey('workspace-preview', 'terminal', sessionId)")).toBe(true);
    expect(terminal.includes('<TerminalSessionContent key={sessionId} sessionId={sessionId} />')).toBe(true);
    expect(transcript.includes("browserStorageKey('workspace-preview', 'execution-attempt'")).toBe(true);
    expect(transcript.includes("browserStorageKey('workspace-preview', 'execution-step'")).toBe(true);
    expect(transcript.includes("browserStorageKey('workspace-preview', 'conversation'")).toBe(true);
    expect(transcript.includes('persistNamespace={transcriptStorageKey}')).toBe(true);
  });
});

describe('preview mixed tab persistence', () => {
  test('does not persist terminal, browser, or workspace tabs', () => {
    const source = readSource(new URL('./PreviewContext.tsx', import.meta.url));

    expect(source.includes("inferPreviewTabKind(tab) === 'file'")).toBe(true);
    expect(source.includes('openTerminalTab')).toBe(true);
    expect(source.includes('openBrowserTab')).toBe(true);
    expect(source.includes('openWorkspaceTab')).toBe(true);
    expect(source.includes('resolveActiveTabAfterClose')).toBe(true);
    expect(source.includes('previousActiveTabIdRef')).toBe(true);
    expect(source.includes("kind: 'workspace'")).toBe(true);
    expect(source.includes('workspaceTabKey: definition.key')).toBe(true);
    expect(source.includes('findWorkspacePreviewTab(prevTabs, definition.key)')).toBe(true);
    expect(source.includes('upsertMixedPreviewTab')).toBe(true);
    expect(source.includes('workspacePath?: string')).toBe(true);
    expect(source.includes("const kind: PreviewTabKind = type === 'url' ? 'browser' : 'file'")).toBe(true);
  });

  test('conversation and terminal surfaces pass workspacePath into the preview provider', () => {
    const chatLayout = readSource(new URL('../../components/ChatLayout/index.tsx', import.meta.url));
    const terminal = readSource(new URL('../../../terminal/TerminalSessionPage.tsx', import.meta.url));

    expect(chatLayout.includes('workspacePath={props.workspacePath}')).toBe(true);
    expect(terminal.includes('workspacePath={session.cwd}')).toBe(true);
  });

  test('desktop preview is a full-height sibling of the header+chat stack', () => {
    const chatLayout = readSource(new URL('../../components/ChatLayout/index.tsx', import.meta.url));

    expect(chatLayout.includes("isMobile ? 'flex-col' : 'flex-row'")).toBe(true);
    expect(chatLayout.includes('rounded-bl-[15px]')).toBe(true);
    expect(chatLayout.includes("borderRight: showToolRail ? 'none' : undefined")).toBe(true);
    expect(chatLayout.includes('{headerBlock}')).toBe(true);
    expect(chatLayout.includes('{chatColumnBody}')).toBe(true);
    const headerAt = chatLayout.indexOf('{headerBlock}');
    const previewAt = chatLayout.indexOf('display: isPreviewOpen ? undefined : \'none\'');
    expect(headerAt).toBeGreaterThan(-1);
    expect(previewAt).toBeGreaterThan(headerAt);
    expect(chatLayout.includes('openWorkspaceTab(definition)')).toBe(true);
    expect(chatLayout.includes('onWorkspaceTabActivate={selectWorkspaceTool}')).toBe(true);
  });
});
